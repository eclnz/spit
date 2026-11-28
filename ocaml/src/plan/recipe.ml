open Ast
open Data

type t = {
  pipeline : string;
  root : string option;
  output : string option;
  paths : (string * pattern) list;
}

let parse text =
  let pipeline = ref None and root = ref None and output = ref None and paths = ref [] in
  let inputs = ref false in
  let assign name slot value =
    if !slot <> None then error "recipe" (name ^ " is declared twice")
    else if value = "" then error "recipe" (name ^ " needs a value")
    else (
      slot := Some value;
      Ok ())
  in
  let read (line : line) =
    let text = Span.text (Span.trim line.span) in
    if Layout.ignored line then Ok ()
    else if line.indent = 0 then (
      inputs := false;
      let directive prefix slot =
        assign prefix slot
          (String.trim
             (String.sub text (String.length prefix) (String.length text - String.length prefix)))
      in
      if String.starts_with ~prefix:"pipeline " text then directive "pipeline " pipeline
      else if String.starts_with ~prefix:"root " text then directive "root " root
      else if String.starts_with ~prefix:"output " text then directive "output " output
      else if text = "inputs:" then (
        inputs := true;
        Ok ())
      else error "recipe" ("unsupported recipe directive: " ^ text))
    else if !inputs then (
      match String.index_opt text ':' with
      | None -> error "recipe" "an input rule needs product: pattern"
      | Some at ->
          let name = String.trim (String.sub text 0 at) in
          let value = String.trim (String.sub text (at + 1) (String.length text - at - 1)) in
          if List.mem_assoc name !paths then error name "input path is declared twice"
          else if value = "" then error name "an input path needs a pattern"
          else
            let span = { source = value; start = 0; finish = String.length value } in
            let* pattern = Inventory.syntax name (Pattern.path span) in
            paths := (name, pattern) :: !paths;
            Ok ())
    else error "recipe" "unexpected indented recipe line"
  in
  let* _ = read_list read (Layout.lines text) in
  match !pipeline with
  | None -> error "recipe" "a recipe needs pipeline FILE"
  | Some pipeline -> Ok { pipeline; root = !root; output = !output; paths = List.rev !paths }

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

let apply_paths checked paths =
  let* overrides =
    read_list
      (fun (name, (pattern : pattern)) ->
        match Hashtbl.find_opt checked.names name with
        | Some id when Array.mem id checked.sources ->
            let product = product checked id in
            if
              List.exists
                (fun (s : Setting.t located) ->
                  match s.value with
                  | Setting.Path (Named_artifact source, _) -> source = name
                  | _ -> false)
                product.settings
            then error name "source has paths in both pipeline and recipe"
            else
              Ok
                ( id,
                  {
                    product with
                    settings =
                      product.settings
                      @ [
                          {
                            span = pattern.span;
                            value = Setting.Path (Named_artifact name, pattern);
                          };
                        ];
                  } )
        | _ -> error name "recipe input path must name a source")
      paths
  in
  let products = Array.copy checked.products in
  List.iter (fun (id, product) -> products.(Product_id.index id) <- product) overrides;
  Ok { checked with products }

let discover checked root =
  let* rules =
    read_list
      (fun id ->
        let product = product checked id in
        let* prepared = Paths.prepare product false in
        let+ parts = parts product.name prepared.pattern in
        let parts =
          match (prepared.extension, prepared.pattern.suffix) with
          | Some (Replace extension), None -> parts @ [ Literal extension ]
          | _ -> parts
        in
        (product, parts))
      (Array.to_list checked.sources)
  in
  let files = files root in
  let* records =
    read_list
      (fun ((product : product), parts) ->
        let found =
          List.filter_map
            (fun path ->
              Option.map
                (fun values -> ({ Inventory.product = product.name; values } : Inventory.record))
                (matching parts path))
            files
        in
        if found = [] then error product.name "source path matched no files" else Ok found)
      rules
  in
  Ok { Inventory.root = Some root; output = None; records = List.concat records }
