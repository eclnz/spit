open Ast
open Data

type t = {
  pipeline : string;
  tools : string option;
  root : string option;
  output : string option;
  paths : (string * pattern) list;
  reviews : (string * string) list;
}

type section = Top | Inputs

type fields = {
  pipeline_file : string option;
  tools_file : string option;
  root_dir : string option;
  output_dir : string option;
  source_paths : (string * pattern) list;
  review_files : (string * string) list;
}

let value name previous text =
  match (previous, text) with
  | Some _, _ -> error (Problem.Recipe_field_twice name)
  | None, "" -> error (Problem.Recipe_field_empty name)
  | None, text -> Ok (Some text)

let directive fields text =
  let word, argument =
    match String.index_opt text ' ' with
    | None -> (text, "")
    | Some at ->
        (String.sub text 0 at, String.trim (String.sub text (at + 1) (String.length text - at - 1)))
  in
  match word with
  | "pipeline" ->
      let+ pipeline_file = value "pipeline " fields.pipeline_file argument in
      { fields with pipeline_file }
  | "tools" when String.starts_with ~prefix:"from " argument ->
      let path = String.trim (String.sub argument 5 (String.length argument - 5)) in
      let+ tools_file = value "tools" fields.tools_file path in
      { fields with tools_file }
  | "root" ->
      let+ root_dir = value "root " fields.root_dir argument in
      { fields with root_dir }
  | "output" ->
      let+ output_dir = value "output " fields.output_dir argument in
      { fields with output_dir }
  | "review" -> (
      match String.split_on_char ' ' argument |> List.filter (( <> ) "") with
      | [ name; "from"; file ] ->
          if List.mem_assoc name fields.review_files then error (Problem.Recipe_review_twice name)
          else Ok { fields with review_files = (name, file) :: fields.review_files }
      | _ -> error (Problem.Recipe_directive text))
  | _ -> error (Problem.Recipe_directive text)

let input_rule paths text =
  match String.index_opt text ':' with
  | None -> error Problem.Recipe_input_rule
  | Some at ->
      let name = String.trim (String.sub text 0 at) in
      let value = String.trim (String.sub text (at + 1) (String.length text - at - 1)) in
      if List.mem_assoc name paths then error (Problem.Recipe_input_twice name)
      else if value = "" then error (Problem.Recipe_input_empty name)
      else
        let span = { source = value; start = 0; finish = String.length value } in
        let+ pattern = Inventory.syntax name (Pattern.path span) in
        (name, pattern) :: paths

let read_line (fields, section) (line : line) =
  let text = Span.text (Span.trim line.span) in
  if Layout.ignored line then Ok (fields, section)
  else
    match (line.indent, section, text) with
    | 0, _, "inputs:" -> Ok (fields, Inputs)
    | 0, _, text ->
        let+ fields = directive fields text in
        (fields, Top)
    | _, Inputs, text ->
        let+ source_paths = input_rule fields.source_paths text in
        ({ fields with source_paths }, Inputs)
    | _ -> error Problem.Recipe_indented

let parse text =
  let initial =
    {
      pipeline_file = None;
      tools_file = None;
      root_dir = None;
      output_dir = None;
      source_paths = [];
      review_files = [];
    }
  in
  let* fields, _ =
    List.fold_left
      (fun state line ->
        let* state = state in
        read_line state line)
      (Ok (initial, Top))
      (Layout.lines text)
  in
  match fields.pipeline_file with
  | None -> error Problem.Recipe_needs_pipeline
  | Some pipeline ->
      Ok
        {
          pipeline;
          tools = fields.tools_file;
          root = fields.root_dir;
          output = fields.output_dir;
          paths = List.rev fields.source_paths;
          reviews = List.rev fields.review_files;
        }

let source checked name =
  match Hashtbl.find_opt checked.names name with
  | Some id when Array.mem id checked.sources -> Ok (id, product checked id)
  | _ -> error (Problem.Recipe_names_no_source name)

let attach_path (product : product) (pattern : pattern) =
  let specific (setting : Setting.t located) =
    match setting.value with
    | Setting.Path (Named_artifact name, _) -> name = product.name
    | _ -> false
  in
  if List.exists specific product.settings then error (Problem.Recipe_path_twice product.name)
  else
    let setting =
      { span = pattern.span; value = Setting.Path (Named_artifact product.name, pattern) }
    in
    Ok { product with settings = product.settings @ [ setting ] }

let apply_paths checked paths =
  let* overrides =
    read_list
      (fun (name, pattern) ->
        let* id, product = source checked name in
        let+ product = attach_path product pattern in
        (id, product))
      paths
  in
  let products = Array.copy checked.products in
  List.iter (fun (id, product) -> products.(Product_id.index id) <- product) overrides;
  Ok { checked with products }
