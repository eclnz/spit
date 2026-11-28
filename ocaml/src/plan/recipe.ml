open Ast
open Data

type t = {
  pipeline : string;
  tools : string option;
  root : string option;
  output : string option;
  paths : (string * pattern) list;
  reviews : (string * string) list;
  contexts : pattern option;
  default : pattern option;
  rules : Recipe_rules.rules;
}

type section = Top | Inputs | Excludes | Requires

type fields = {
  pipeline_file : string option;
  tools_file : string option;
  root_dir : string option;
  output_dir : string option;
  source_paths : (string * pattern) list;
  review_files : (string * string) list;
  context_pattern : pattern option;
  default_pattern : pattern option;
  rule_set : Recipe_rules.rules;
}

let value name previous text =
  match (previous, text) with
  | Some _, _ -> error (Problem.Recipe_field_twice name)
  | None, "" -> error (Problem.Recipe_field_empty name)
  | None, text -> Ok (Some text)

let add_exclusion fields line text =
  let+ exclusion = Recipe_rules.exclusion line text in
  {
    fields with
    rule_set = { fields.rule_set with exclusions = fields.rule_set.exclusions @ [ exclusion ] };
  }

let add_requirement fields line text =
  let+ requirement = Recipe_rules.requirement line text in
  {
    fields with
    rule_set =
      { fields.rule_set with requirements = fields.rule_set.requirements @ [ requirement ] };
  }

let directive fields line text =
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
  | "exclude" -> add_exclusion fields line argument
  | "require" -> add_requirement fields line argument
  | "prefer" ->
      let+ preference = Recipe_rules.preference line argument in
      {
        fields with
        rule_set =
          { fields.rule_set with preferences = fields.rule_set.preferences @ [ preference ] };
      }
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

let pattern_of name value =
  let value = String.trim value in
  let span = { source = value; start = 0; finish = String.length value } in
  Inventory.syntax name (Pattern.path span)

let special fields name value =
  match name with
  | "contexts" ->
      if fields.context_pattern <> None then error Problem.Contexts_twice
      else
        let+ pattern = pattern_of name value in
        Some { fields with context_pattern = Some pattern }
  | "default" ->
      if fields.default_pattern <> None then error Problem.Recipe_default_twice
      else
        let+ pattern = pattern_of name value in
        Some { fields with default_pattern = Some pattern }
  | _ -> Ok None

let read_line index (fields, section) (line : line) =
  let text = Span.text (Span.trim line.span) in
  let number = fst (Source.position index line.span.start) in
  if Layout.ignored line then Ok (fields, section)
  else
    match (line.indent, section, text) with
    | 0, _, "inputs:" -> Ok (fields, Inputs)
    | 0, _, "exclude:" -> Ok (fields, Excludes)
    | 0, _, "require:" -> Ok (fields, Requires)
    | 0, _, text ->
        let+ fields = directive fields number text in
        (fields, Top)
    | _, Inputs, text -> (
        let name, value =
          match String.index_opt text ':' with
          | Some at ->
              ( String.trim (String.sub text 0 at),
                String.sub text (at + 1) (String.length text - at - 1) )
          | None -> (text, "")
        in
        let* handled = special fields name value in
        match handled with
        | Some fields -> Ok (fields, Inputs)
        | None ->
            let+ source_paths = input_rule fields.source_paths text in
            ({ fields with source_paths }, Inputs))
    | _, Excludes, text ->
        let+ fields = add_exclusion fields number text in
        (fields, Excludes)
    | _, Requires, text ->
        let+ fields = add_requirement fields number text in
        (fields, Requires)
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
      context_pattern = None;
      default_pattern = None;
      rule_set = Recipe_rules.empty;
    }
  in
  let index = Source.index text in
  let* fields, _ =
    List.fold_left
      (fun state line ->
        let* state = state in
        read_line index state line)
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
          contexts = fields.context_pattern;
          default = fields.default_pattern;
          rules = fields.rule_set;
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

let apply_default checked (pattern : pattern option) =
  match pattern with
  | None -> checked
  | Some pattern ->
      let products =
        Array.map
          (fun (product : product) ->
            let own =
              List.exists
                (fun (setting : Setting.t located) ->
                  match setting.value with
                  | Setting.Path (Named_artifact name, _) -> name = product.name
                  | _ -> false)
                product.settings
            in
            if Array.mem product.id checked.sources && not own then
              {
                product with
                settings =
                  product.settings
                  @ [
                      {
                        span = pattern.span;
                        value = Setting.Path (Named_artifact product.name, pattern);
                      };
                    ];
              }
            else product)
          checked.products
      in
      { checked with products }

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
