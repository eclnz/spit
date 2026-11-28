open Ast
open Data

type part = Literal of string | Capture of string * value_shape option * char list

let entity_parts (rule : Paths.rule) symbols (product : product) =
  match rule.format with
  | None -> error (Problem.Entities_without_format product.name)
  | Some format ->
      let reserved = Paths.reserved format in
      let one dim =
        let name = symbol_text symbols dim in
        let key = Option.value (Hashtbl.find_opt rule.keys name) ~default:name in
        List.filter_map
          (function
            | Pattern_text text -> Some (Literal text.value)
            | Pattern_value { value = Entity_key; _ } -> Some (Literal (Paths.encode ~reserved key))
            | Pattern_value { value = Entity_value; _ } -> Some (Capture (name, None, reserved))
            | _ -> None)
          format.pattern.segments
      in
      Ok
        (List.concat
           (List.mapi
              (fun i dim -> if i = 0 then one dim else Literal format.separator :: one dim)
              (Array.to_list product.dims)))

let parts checked (product : product) (rule : Paths.rule) =
  let segments, _ = Path_check.flatten checked.symbols product rule.pattern.segments in
  let+ parts =
    read_list
      (function
        | Pattern_text text -> Ok [ Literal text.value ]
        | Pattern_value { value = Dimension_name (name, shape); _ } ->
            Ok [ Capture (name, shape, []) ]
        | Pattern_value { value = Product_name; _ } -> Ok [ Literal product.path_name ]
        | Pattern_value { value = Stage_name; _ } ->
            Ok [ Literal (String.concat "/" product.stage) ]
        | Pattern_value { value = Entity_names; _ } -> entity_parts rule checked.symbols product
        | _ -> error (Problem.Discovery_placeholder product.name))
      segments
  in
  List.concat parts

let matching parts path =
  let length = String.length path in
  let rec matches parts offset values =
    match parts with
    | [] -> if offset = length then Some values else None
    | Literal text :: rest ->
        let size = String.length text in
        if offset + size <= length && String.sub path offset size = text then
          matches rest (offset + size) values
        else None
    | Capture (name, shape, reserved) :: rest ->
        let limit =
          match String.index_from_opt path offset '/' with None -> length | Some at -> at
        in
        let fixed = Option.bind shape Shape.fixed_length in
        let fits value =
          (not (String.exists (fun c -> List.mem c reserved) value))
          && Option.fold ~none:true
               ~some:(fun shape -> Shape.accepts shape (Paths.decode value))
               shape
        in
        let rec attempt finish =
          let next () = if fixed = None then attempt (finish + 1) else None in
          if finish > limit then None
          else
            let value = String.sub path offset (finish - offset) in
            match List.assoc_opt name values with
            | Some previous when previous <> value -> next ()
            | _ when not (fits value) -> next ()
            | _ -> (
                let values =
                  if List.mem_assoc name values then values else (name, value) :: values
                in
                match matches rest finish values with Some _ as found -> found | None -> next ())
        in
        attempt (offset + Option.value fixed ~default:1)
  in
  Option.map (List.map (fun (name, value) -> (name, Paths.decode value))) (matches parts 0 [])

let entries root =
  let pending = Stack.create () and found = ref [] and folders = ref [] in
  Stack.push "" pending;
  while not (Stack.is_empty pending) do
    let relative = Stack.pop pending in
    let directory = Filename.concat root relative in
    Array.iter
      (fun name ->
        let path = if relative = "" then name else relative ^ "/" ^ name in
        let full = Filename.concat root path in
        if Sys.is_directory full then (
          if
            (not (List.mem name [ ".git"; ".spit-bash"; "_build"; "target" ]))
            && (Unix.lstat full).Unix.st_kind <> Unix.S_LNK
          then (
            folders := path :: !folders;
            Stack.push path pending))
        else found := path :: !found)
      (Sys.readdir directory)
  done;
  (List.sort String.compare !found, List.sort String.compare !folders)

let source_rule checked (product : product) =
  let* prepared =
    match Path_check.rule checked product with
    | Some prepared -> Ok prepared
    | None -> error (Problem.Source_no_path_rule product.name)
  in
  let+ pattern = parts checked product prepared in
  let pattern =
    match (prepared.extension, prepared.pattern.suffix) with
    | Some (Replace extension), None -> pattern @ [ Literal extension ]
    | _ -> pattern
  in
  (product.name, pattern)

let source_records files (name, pattern) =
  let records =
    List.filter_map
      (fun path ->
        Option.map
          (fun values -> (path, ({ Inventory.product = name; values } : Inventory.record)))
          (matching pattern path))
      files
  in
  match records with [] -> error (Problem.No_files name) | records -> Ok records

let unclaimed found =
  let owners = Hashtbl.create 64 in
  let+ _ =
    read_list
      (fun (path, (record : Inventory.record)) ->
        match Hashtbl.find_opt owners path with
        | Some other when other <> record.product ->
            error (Problem.Claimed_twice { path; first = other; second = record.product })
        | _ ->
            Hashtbl.replace owners path record.product;
            Ok ())
      found
  in
  List.map snd found

let referring (product : product) =
  match rule_pattern product.name product.settings with
  | Some pattern -> references pattern.segments
  | None -> []

let entities checked (record : Inventory.record) =
  Array.of_list
    (List.map
       (fun (dim, value) -> (intern checked.symbols dim, intern checked.symbols value))
       record.values)

let companions checked root found (product : product) =
  let* rule =
    match Path_check.rule checked product with
    | Some rule -> Ok rule
    | None -> error (Problem.Source_no_path_rule product.name)
  in
  let anchor = List.hd (referring product) in
  let* anchor_rule =
    let anchor_product = Data.product checked (Hashtbl.find checked.names anchor) in
    match Path_check.rule checked anchor_product with
    | Some rule -> Ok rule
    | None -> error (Problem.Source_no_path_rule anchor)
  in
  read_list
    (fun ((anchor_path : string), (record : Inventory.record)) ->
      let refer _ = Ok (anchor_path, Paths.final_extension anchor_rule) in
      let* path = Paths.render ~refer rule checked.symbols (entities checked record) in
      if Sys.file_exists (Filename.concat root path) then
        Ok (path, ({ product = product.name; values = record.values } : Inventory.record))
      else error (Problem.Companion_missing { product = product.name; path }))
    (List.filter (fun (_, (record : Inventory.record)) -> record.product = anchor) found)

let discover ?pipeline_folder checked root =
  let sources = List.map (product checked) (Array.to_list checked.sources) in
  let plain, dependent = List.partition (fun p -> referring p = []) sources in
  let* rules = read_list (source_rule checked) plain in
  let dataset_files, dataset_folders = entries root in
  let* records =
    read_list
      (fun ((name, _) as rule) ->
        let id = Hashtbl.find checked.names name in
        if (product checked id).here then
          let folder = Option.value pipeline_folder ~default:root in
          let* prepared = Paths.prepare (product checked id) false in
          let* path = Paths.render prepared checked.symbols [||] in
          let full = Filename.concat folder path in
          if Sys.file_exists full && not (Sys.is_directory full) then source_records [ path ] rule
          else error (Problem.Local_file_missing name)
        else if (product checked id).shape = Folder then source_records dataset_folders rule
        else source_records dataset_files rule)
      rules
  in
  let found = List.concat records in
  let* companions = read_list (companions checked root found) dependent in
  let+ records = unclaimed (found @ List.concat companions) in
  { Inventory.root = Some root; output = None; records }
