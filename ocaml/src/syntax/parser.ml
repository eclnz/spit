open Ast
open Span
open Grammar
open Layout

let read_source name_span name (e : entry) =
  let* span = single e in
  let* description = describe span in
  source name_span name span description

let read_use (e : entry) =
  let* alias =
    if empty e.head then Ok None
    else
      let+ name = check_name "`use`" e.head in
      Some name
  in
  let* source =
    match e.content with
    | Value value -> Ok value
    | _ -> at e.header.rest (Missing_import_source (text e.header.rest))
  in
  let* tokens = words source in
  match tokens with
  | [ path ] -> Ok { span = e.head; names = None; from = text path; alias }
  | [ path; names ] ->
      let* names = enclosed '[' ']' names in
      let+ names = read_list (check_name "an imported name") names in
      { span = e.head; names = Some names; from = text path; alias }
  | _ -> at source (Invalid_import (text source))

let gate_target (e : entry) =
  if empty e.head then Ok Every_artifact
  else
    let* ext = extension e.head in
    match ext with
    | Replace ext -> Ok (Extension ext)
    | Append _ -> at e.head (Invalid_extension (text e.head))

let read_default_gate kind (e : entry) =
  let* target = gate_target e in
  let* span = single e in
  let* uses = comma_list span in
  let+ uses = read_list read_gate_use uses in
  Setting.Gate (kind, target, uses)

let read_script (e : entry) =
  match e.content with
  | Script_body (span, indent) -> Template.script ~indent span
  | Value span -> Template.script ~indent:0 span
  | _ -> at e.head (Missing_script (e.header.rule.word, text e.head))

let read_gate_definition kind (e : entry) =
  let* name, parameters = applied e.head in
  let* name = check_name (owner e) name in
  let* params = read_list (check_name "a parameter") parameters in
  match (kind, params, e.content) with
  | Gate_kind.Review, _ :: _, _ -> at e.head (Review_parameters name)
  | Review, [], Bare -> Ok (Review name)
  | Review, [], _ -> at e.head (Review_script (name, content_text e))
  | (Check | Screen), _, _ ->
      let* script = read_script e in
      if empty script.span then at e.head (Gate_needs_script (kind, name, text e.head))
      else
        let test = { name; params; script } in
        Ok (if kind = Check then Check test else Screen test)

let setting (e : entry) value = Setting { span = e.head; value }

let read_value_setting read_target parse_value constructor (e : entry) =
  let* target = read_target e in
  let* span = single e in
  if empty span then at span Empty_value
  else
    let+ value = parse_value span in
    setting e (constructor target value)

let read_script_setting constructor (e : entry) =
  let* target = operation_target e in
  let* script = read_script e in
  if empty script.span then at e.head (Missing_script (e.header.rule.word, text e.head))
  else Ok (setting e (constructor target script))

let read_resources span =
  let* pairs = comma_list span in
  read_list
    (fun input ->
      let* key, value = separated "=" input in
      let* key = check_name "a resource" key in
      if empty value then at value Empty_value else Ok (key, text value))
    pairs

let read_entities (e : entry) =
  let* target = dimension_target e in
  let* span = single e in
  let* value =
    match target with
    | Every_dimension ->
        let+ format = Pattern.entities span in
        Format format
    | Named_dimension _ ->
        let+ name = check_name "a dimension spelling" span in
        Rename name
  in
  Ok (setting e (Entities (target, value)))

let read_anchored (e : entry) =
  let* target =
    if e.header.default then
      let* () =
        match e.content with
        | Bare -> Ok ()
        | _ -> at e.header.rest (Expected_name_only ("default anchored", content_text e))
      in
      operation_target e
    else
      let* span = name_only e in
      let+ name = check_name (owner e) span in
      Named_operation name
  in
  Ok (setting e (Anchored target))

let read_gate kind (e : entry) =
  if e.header.default then
    let+ value = read_default_gate kind e in
    setting e value
  else
    let+ gate = read_gate_definition kind e in
    Gate_def gate

let plain_header input =
  let* line, rest = read_until 0 [ "\r"; "\n" ] input in
  let+ head, value = split_entry line in
  { syntax = Plain_header; head; value; rest }

let finish_header syntax input rest allows_body =
  let rest = horizontal rest in
  let head = trim (slice input.start rest.start input) in
  if allows_body && starts ":" rest then
    let+ value, rest = read_until 0 [ "\r"; "\n" ] (drop 1 rest) in
    { syntax; head; value = Some value; rest }
  else if empty rest || starts "\r" rest || starts "\n" rest then
    Ok { syntax; head; value = None; rest }
  else at rest (Unexpected_text (text rest))

let read_syntax keyword input =
  match keyword with
  | Keyword.Operation ->
      let* operation, rest = read_operation_prefix input in
      finish_header (Operation_header operation) input rest true
  | Product ->
      let* binding, rest = read_product_prefix input in
      finish_header (Product_header binding) input rest false
  | Type ->
      let* definition, rest = read_type_def_prefix input in
      finish_header (Type_header definition) input rest false
  | Source ->
      let* syntax, rest = read_source_prefix input in
      finish_header syntax input rest true
  | (Root | Output) when not (starts ":" input) ->
      let+ value, rest = read_until 0 [ "\r"; "\n" ] input in
      {
        syntax = Plain_header;
        head = slice input.start input.start input;
        value = Some value;
        rest;
      }
  | _ -> plain_header input

type context = File_scope | Stage_scope | Operation_body

let validate_context context (e : entry) =
  if e.header.default && not e.header.rule.allows_default then
    at e.head (Disallowed_default e.header.rule.word)
  else if context = Operation_body && e.header.rule.keyword <> Keyword.Product then
    at e.head Composite_statement
  else if context = Stage_scope && e.header.rule.top_level && not e.header.default then
    at e.head Nested_definition
  else Ok ()

let read_types span =
  match text span with
  | "standard" -> Ok Standard
  | "strict" -> Ok Strict
  | s -> at span (Invalid_types_mode s)

let rec read_scope depth context pending =
  let singletons = ref [] in
  let read entry =
    let* e = entry in
    let* () = validate_context context e in
    let* value = read_statement depth e in
    let key = e.header.rule.word in
    if e.header.rule.singleton && List.mem key !singletons then at e.head (Duplicate_setting key)
    else (
      if e.header.rule.singleton then singletons := key :: !singletons;
      Ok { span = e.head; value })
  in
  { entries = List.map read (entries read_syntax None false pending) }

and read_statement depth (e : entry) =
  let item constructor value =
    let+ value = value in
    constructor value
  in
  match e.header.rule.keyword with
  | Keyword.Types ->
      item
        (fun t -> Types t)
        (let* span = value_only e in
         read_types span)
  | Type ->
      item
        (fun t -> Type_def t)
        (match e.syntax with
        | Type_header definition -> Ok definition
        | _ -> at e.head (Unexpected_text (text e.head)))
  | Use -> item (fun t -> Import t) (read_use e)
  | Root ->
      item
        (fun t -> Root t)
        (let+ span = value_only e in
         text span)
  | Output ->
      item
        (fun t -> Output_dir t)
        (let+ span = value_only e in
         text span)
  | Dimensions ->
      item
        (fun t -> Dimensions t)
        (let* span = value_only e in
         let* dims = enclosed '[' ']' span in
         read_list (check_name "a dimension") dims)
  | Tool ->
      item
        (fun t -> Tool t)
        (let* span = name_only e in
         check_name (owner e) span)
  | Source ->
      let+ source =
        match e.syntax with
        | Source_header source -> Ok source
        | Source_body_header (span, name) -> read_source span name e
        | _ -> at e.head (Unexpected_text (text e.head))
      in
      Source_declaration source
  | Product -> (
      match e.syntax with
      | Product_header binding -> Ok (Product_binding binding)
      | _ -> at e.head (Binding_description (content_text e)))
  | Check -> read_gate Gate_kind.Check e
  | Screen -> read_gate Gate_kind.Screen e
  | Review -> read_gate Gate_kind.Review e
  | Path -> read_value_setting artifact_target Pattern.path (fun t v -> Setting.Path (t, v)) e
  | Ext -> read_value_setting artifact_target extension (fun t v -> Setting.Ext (t, v)) e
  | Entities -> read_entities e
  | Command -> read_script_setting (fun t v -> Setting.Command (t, v)) e
  | Verify -> read_script_setting (fun t v -> Setting.Verify (t, v)) e
  | Calculate ->
      read_value_setting operation_target
        (fun span -> Ok { span; value = text span })
        (fun t v -> Setting.Calculate (t, v))
        e
  | Resources ->
      read_value_setting operation_target read_resources (fun t v -> Setting.Resources (t, v)) e
  | Uses ->
      read_value_setting operation_target
        (fun s ->
          let* uses = comma_list s in
          read_list read_tool_use uses)
        (fun t v -> Setting.Uses (t, v))
        e
  | Anchored -> read_anchored e
  | Stage -> (
      let* name = check_name (owner e) e.head in
      let* body = read_body depth Stage_scope e in
      match body with
      | Some scope -> Ok (Stage_declaration (name, scope))
      | None -> at e.head Expected_stage_body)
  | Operation -> read_operation depth e

and read_body depth context (e : entry) =
  match e.content with
  | Body _ when depth >= max_nesting -> at e.head Nesting_limit
  | Body lines -> Ok (Some (read_scope (depth + 1) context lines))
  | Bare -> Ok None
  | _ -> at e.head (Unexpected_body_value (text e.head, content_text e))

and read_operation depth (e : entry) =
  let* operation =
    match e.syntax with
    | Operation_header operation -> Ok operation
    | _ -> at e.head (Unexpected_text (text e.head))
  in
  let* body = read_body depth Operation_body e in
  let* () =
    match body with
    | None -> Ok ()
    | Some _ ->
        if
          operation.outputs <> []
          && List.for_all
               (fun (o : output annotated) -> o.value.name <> None && o.value.shape = File None)
               operation.outputs
        then Ok ()
        else at e.head Composite_output
  in
  Ok (Operation_declaration { operation with body })

let gates_on target annotations =
  List.map
    (fun (annotation : gate_annotation) ->
      { span = annotation.span; value = Setting.Gate (annotation.kind, target, annotation.uses) })
    annotations

let empty_pipeline =
  {
    types = None;
    root = None;
    output_dir = None;
    imports = [];
    type_defs = [];
    dimensions = [];
    tools = [];
    gates = [];
    global = Scope.empty;
  }

type lowering = { mutable diagnostics : diagnostic list; mutable job : int }

let body_bindings state (scope : parsed_scope) =
  List.filter_map
    (function
      | Error d ->
          state.diagnostics <- d :: state.diagnostics;
          None
      | Ok ({ value = Product_binding binding; _ } : statement located) -> Some binding
      | Ok _ -> None)
    scope.entries

let lower_source global (source : artifact annotated) =
  {
    global with
    artifacts = source.value :: global.artifacts;
    settings =
      List.rev_append (gates_on (Named_artifact source.value.name) source.gates) global.settings;
  }

let lower_operation state global (operation : parsed_operation) =
  let declaration : operation_decl =
    {
      span = operation.span;
      name = operation.name;
      params = List.map (fun (p : param annotated) -> p.value) operation.params;
      outputs = List.map (fun (o : output annotated) -> o.value) operation.outputs;
      body = Option.map (body_bindings state) operation.body;
    }
  in
  let gates =
    List.concat_map
      (fun (p : param annotated) -> gates_on (Input_of (operation.name, p.value.name)) p.gates)
      operation.params
    @ List.concat_map
        (fun (o : output annotated) -> gates_on (Output_of (operation.name, o.value.name)) o.gates)
        operation.outputs
  in
  {
    global with
    operations = declaration :: global.operations;
    settings = List.rev_append gates global.settings;
  }

let rec lower_scope (scope : parsed_scope) =
  let state = { diagnostics = []; job = 0 } in
  let pipeline = List.fold_left (lower_entry state) empty_pipeline scope.entries in
  ( {
      pipeline with
      imports = List.rev pipeline.imports;
      type_defs = List.rev pipeline.type_defs;
      dimensions = List.rev pipeline.dimensions;
      tools = List.rev pipeline.tools;
      gates = List.rev pipeline.gates;
      global = Scope.finish_reversed pipeline.global;
    },
    List.rev state.diagnostics )

and lower_entry state pipeline (entry : (statement located, diagnostic) result) =
  let global = pipeline.global in
  match entry with
  | Error d ->
      state.diagnostics <- d :: state.diagnostics;
      pipeline
  | Ok { value; _ } -> (
      match value with
      | Types types -> { pipeline with types = Some types }
      | Root root -> { pipeline with root = Some root }
      | Output_dir output_dir -> { pipeline with output_dir = Some output_dir }
      | Import import -> { pipeline with imports = import :: pipeline.imports }
      | Type_def definition -> { pipeline with type_defs = definition :: pipeline.type_defs }
      | Dimensions dims -> { pipeline with dimensions = List.rev_append dims pipeline.dimensions }
      | Tool tool -> { pipeline with tools = tool :: pipeline.tools }
      | Gate_def gate -> { pipeline with gates = gate :: pipeline.gates }
      | Setting setting ->
          { pipeline with global = { global with settings = setting :: global.settings } }
      | Source_declaration source -> { pipeline with global = lower_source global source }
      | Product_binding binding ->
          let global = Scope.prepend_binding state.job global binding in
          state.job <- state.job + 1;
          { pipeline with global }
      | Stage_declaration (name, body) ->
          let child, ds = lower_scope body in
          state.diagnostics <- List.rev_append ds state.diagnostics;
          {
            pipeline with
            global = { global with stages = { name; scope = child.global } :: global.stages };
          }
      | Operation_declaration operation ->
          { pipeline with global = lower_operation state global operation })

let default_key = function
  | Setting.Verify (Every_operation, _) -> Some "default verify"
  | Calculate (Every_operation, _) -> Some "default calculate"
  | Resources (Every_operation, _) -> Some "default resources"
  | Uses (Every_operation, _) -> Some "default uses"
  | Anchored Every_operation -> Some "default anchored"
  | Path (Every_artifact, _) -> Some "default path"
  | Ext (Every_artifact, _) -> Some "default ext"
  | Entities (Every_dimension, _) -> Some "default entities"
  | Gate (kind, Every_artifact, _) -> Some ("default " ^ gate_word kind)
  | Gate (kind, Extension ext, _) -> Some ("default " ^ gate_word kind ^ " " ^ ext)
  | _ -> None

let rec scope_diagnostics scope =
  let seen = Hashtbl.create 16 in
  let diagnostics =
    List.fold_left
      (fun diagnostics (s : Setting.t located) ->
        match default_key s.value with
        | None -> diagnostics
        | Some key when Hashtbl.mem seen key ->
            { span = s.span; problem = Duplicate_setting key } :: diagnostics
        | Some key ->
            Hashtbl.add seen key ();
            diagnostics)
      [] scope.settings
  in
  List.rev diagnostics @ List.concat_map (fun (s : stage) -> scope_diagnostics s.scope) scope.stages

let parse source =
  let parsed = read_scope 0 File_scope (lines source) in
  let pipeline, diagnostics = lower_scope parsed in
  let diagnostics = diagnostics @ scope_diagnostics pipeline.global in
  ( pipeline,
    List.stable_sort (fun (a : diagnostic) b -> Int.compare a.span.start b.span.start) diagnostics
  )
