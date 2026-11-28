open Ast
open Data

type part = Literal of string | Capture of string

let parts product pattern =
  read_list
    (function
      | Pattern_text text -> Ok (Literal text.value)
      | Pattern_value { value = Dimension_name name; _ } -> Ok (Capture name)
      | Pattern_value { value = Product_name; _ } -> Ok (Literal product)
      | _ -> error product "source discovery supports literal text and dimension placeholders")
    pattern.segments

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
    | Capture name :: rest ->
        let limit =
          match String.index_from_opt path offset '/' with None -> length | Some at -> at
        in
        let rec attempt finish =
          if finish > limit then None
          else
            let value = String.sub path offset (finish - offset) in
            match List.assoc_opt name values with
            | Some previous when previous <> value -> attempt (finish + 1)
            | _ -> (
                let values =
                  if List.mem_assoc name values then values else (name, value) :: values
                in
                match matches rest finish values with
                | Some _ as found -> found
                | None -> attempt (finish + 1))
        in
        attempt (offset + 1)
  in
  matches parts 0 []

let files root =
  let pending = Stack.create () and found = ref [] in
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
          then Stack.push path pending)
        else found := path :: !found)
      (Sys.readdir directory)
  done;
  List.sort String.compare !found

let source_rule (product : product) =
  let* prepared = Paths.prepare product false in
  let+ pattern = parts product.name prepared.pattern in
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
          (fun values -> ({ Inventory.product = name; values } : Inventory.record))
          (matching pattern path))
      files
  in
  match records with [] -> error name "source path matched no files" | records -> Ok records

let discover ?pipeline_folder checked root =
  let* rules =
    read_list (fun id -> source_rule (product checked id)) (Array.to_list checked.sources)
  in
  let dataset_files = files root in
  let+ records =
    read_list
      (fun ((name, _) as rule) ->
        let id = Hashtbl.find checked.names name in
        if (product checked id).here then
          let folder = Option.value pipeline_folder ~default:root in
          let* prepared = Paths.prepare (product checked id) false in
          let* path = Paths.render prepared checked.symbols [||] in
          let full = Filename.concat folder path in
          if Sys.file_exists full && not (Sys.is_directory full) then source_records [ path ] rule
          else error name "pipeline-local source file is missing"
        else source_records dataset_files rule)
      rules
  in
  { Inventory.root = Some root; output = None; records = List.concat records }
