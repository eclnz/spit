open Ast
open Span

let type_name span =
  if empty span || not (upper span.source.[span.start]) then at span (Invalid_type_name (text span))
  else check_name "a type" span

let qualified_type_name input =
  let* first, rest = name_prefix input in
  if starts "." rest then
    let* namespace = check_name "a namespace" first in
    let* last, rest = name_prefix (drop 1 rest) in
    let+ name = type_name last in
    ({ namespace = Some namespace; name }, rest)
  else
    let+ name = type_name first in
    ({ namespace = None; name }, rest)

let rec read_type_prefix depth input =
  if depth > max_nesting then at input Nesting_limit
  else
    let* input = trivia input in
    if starts "$" input then
      let* name, rest = name_prefix (drop 1 input) in
      let+ name = check_name "a type variable" name in
      (Var name, rest)
    else
      let* name, rest = qualified_type_name input in
      if starts "<" rest then
        let+ args, rest = comma_items (Some ">") (read_type_prefix (depth + 1)) (drop 1 rest) in
        (Named (name, args), rest)
      else Ok (Named (name, []), rest)

let read_type input =
  let* value, rest = read_type_prefix 0 input in
  let+ () = require_end rest in
  value

let type_starts input =
  (not (empty input))
  && (upper input.source.[input.start]
     || starts "$" input
     ||
     match name_prefix input with
     | Ok (_, rest) when starts "." rest ->
         rest.start + 1 < rest.finish && upper rest.source.[rest.start + 1]
     | _ -> false)

let read_optional_type input =
  let* input = trivia input in
  if type_starts input then
    let+ value, rest = read_type_prefix 0 input in
    (Some value, rest)
  else Ok (None, input)

let require_type_boundary typ rest =
  if
    typ <> None
    && (not (empty rest))
    && (not (whitespace rest.source.[rest.start]))
    && not (starts "," rest)
  then at rest (Unexpected_text (text rest))
  else Ok ()

let read_gate_use span =
  let negated = starts "!" span in
  let* name, args = applied (if negated then drop 1 span else span) in
  let+ gate = read_ref name in
  { gate; args = List.map text args; negated }

let read_annotation invalid span =
  if not (starts "@" span) then at span (invalid (text span))
  else
    let+ name, args = applied (drop 1 span) in
    (text name, args)

let gate_kind name = match keyword name with Some { form = Gate kind; _ } -> Some kind | _ -> None

let read_tool_use span =
  let negated = starts "!" span in
  let+ tool = check_name "a tool" (if negated then drop 1 span else span) in
  { tool; used = not negated }

let extension span =
  let append = starts "+." span in
  let value = if append then text (drop 1 span) else text span in
  if
    String.length value < 2
    || value.[0] <> '.'
    || List.exists (( = ) "")
         (String.split_on_char '.' (String.sub value 1 (String.length value - 1)))
    || not
         (String.for_all
            (function '.' | '-' | '_' | 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' -> true | _ -> false)
            value)
  then at span (Invalid_extension (text span))
  else Ok (if append then Append value else Replace value)

let read_shape tokens =
  let* ext, tokens =
    match tokens with
    | value :: rest when starts "." value || starts "+." value ->
        let+ ext = extension value in
        (Some ext, rest)
    | rest -> Ok (None, rest)
  in
  match (ext, tokens) with
  | None, slash :: from :: name :: rest when is_token "/" slash && is_token "from" from ->
      let+ name = check_name "`from`" name in
      (Folder_from name, rest)
  | None, slash :: view :: rest when is_token "/" slash && is_token "view" view -> Ok (View, rest)
  | None, slash :: rest when is_token "/" slash -> Ok (Folder, rest)
  | Some _, slash :: _ when is_token "/" slash -> at slash Folder_extension
  | ext, rest -> Ok (File ext, rest)

let read_placement tokens =
  let beside name suffix rest =
    let+ of_ = check_name "`beside`" name in
    (Some (Beside { of_; suffix }), rest)
  in
  match tokens with
  | dims :: rest when starts "[" dims ->
      let* dims = enclosed '[' ']' dims in
      let+ dims = read_list (check_name "a dimension") dims in
      (Some (Dims dims), rest)
  | word :: name :: suffix :: value :: rest when is_token "beside" word && is_token "suffix" suffix
    ->
      let value = text value in
      let suffix =
        if String.length value >= 2 && value.[0] = '"' && value.[String.length value - 1] = '"' then
          String.sub value 1 (String.length value - 2)
        else value
      in
      beside name (Some suffix) rest
  | word :: name :: rest when is_token "beside" word -> beside name None rest
  | word :: rest when is_token "here" word -> Ok (Some Here, rest)
  | rest -> Ok (None, rest)

let read_gate_annotation span kind args =
  if args = [] then at span (Empty_gate_annotation kind)
  else
    let+ uses = read_list read_gate_use args in
    { span; kind; uses }

let read_description_gate span =
  let* name, args = read_annotation (fun s -> Description_order s) span in
  match gate_kind name with
  | Some kind -> read_gate_annotation span kind args
  | None -> at span (Description_order (text span))

let validate_gates annotations =
  let rec check seen = function
    | [] -> Ok annotations
    | annotation :: rest ->
        if List.mem annotation.kind seen then
          at annotation.span (Repeated_gate_annotation annotation.kind)
        else
          let signs = Hashtbl.create 16 in
          let* _ =
            read_list
              (fun use ->
                match Hashtbl.find_opt signs use.gate with
                | Some sign when sign <> use.negated ->
                    at annotation.span (Contradicted_gate use.gate.name)
                | _ ->
                    Hashtbl.replace signs use.gate use.negated;
                    Ok ())
              annotation.uses
          in
          check (annotation.kind :: seen) rest
  in
  check [] annotations

let read_description_prefix ?(stops = [ "," ]) input =
  let* typ, rest = read_optional_type input in
  let* () = require_type_boundary typ rest in
  let* modifiers, remaining = read_until 0 stops rest in
  let* tokens = words modifiers in
  let* shape, tokens = read_shape tokens in
  let* placement, tokens = read_placement tokens in
  let* annotations = read_list read_description_gate tokens in
  let+ gates = validate_gates annotations in
  ({ typ; shape; placement; gates }, remaining)

let describe input =
  let* description, rest = read_description_prefix input in
  let+ () = require_end rest in
  description

let read_output_prefix ?(stops = [ "," ]) input =
  let* head, after_name = name_prefix input in
  let after_name = horizontal after_name in
  let* name, rest =
    if starts ":" after_name then
      let+ name = check_name "an output" head in
      (Some name, drop 1 after_name)
    else Ok (None, input)
  in
  let* description, remaining = read_description_prefix ~stops rest in
  let* beside =
    match description.placement with
    | None -> Ok None
    | Some (Beside b) -> Ok (Some b)
    | _ -> at rest Output_placement
  in
  let output =
    {
      span = trim (slice input.start remaining.start input);
      name;
      typ = description.typ;
      shape = description.shape;
      beside;
    }
  in
  Ok ({ value = output; gates = description.gates }, remaining)

type parameter_annotation =
  | Parameter_constraint of constraint_
  | Parameter_gates of gate_annotation

let read_minimum number =
  match Int32.of_string_opt (text number) with
  | Some n -> Ok (Parameter_constraint (Min (Int32.to_int n)))
  | None -> at number (Invalid_minimum (text number))

let read_parameter_annotation span =
  let* name, args = read_annotation (fun s -> Invalid_parameter_annotation s) span in
  match (name, args, gate_kind name) with
  | "min", [ number ], _ -> read_minimum number
  | _, _, Some kind ->
      let+ gate = read_gate_annotation span kind args in
      Parameter_gates gate
  | _ -> at span (Invalid_parameter_annotation (text span))

let read_param_prefix input =
  let* head, rest = read_until 0 [ ":"; "," ] input in
  let* name = check_name "a parameter" head in
  if not (starts ":" rest) then
    Ok
      ( { value = { span = head; name; many = false; typ = None; constraints = [] }; gates = [] },
        rest )
  else
    let* value = trivia (drop 1 rest) in
    let* first, after_first = name_prefix value in
    let many = is_token "many" first in
    let* typ, rest = read_optional_type (if many then after_first else value) in
    let* () = require_type_boundary typ rest in
    let* annotations, remaining = read_until 0 [ "," ] rest in
    let annotations_span = annotations in
    let* tokens = words annotations in
    let* annotations = read_list read_parameter_annotation tokens in
    let constraints =
      List.filter_map (function Parameter_constraint c -> Some c | _ -> None) annotations
    in
    let* () = if List.length constraints > 1 then at annotations_span Repeated_minimum else Ok () in
    let* gates =
      validate_gates
        (List.filter_map (function Parameter_gates g -> Some g | _ -> None) annotations)
    in
    Ok
      ( {
          value =
            { span = trim (slice input.start remaining.start input); name; many; typ; constraints };
          gates;
        },
        remaining )

let read_operation_prefix input =
  let* name_span, rest = read_until 0 [ "("; "->"; "\r"; "\n" ] input in
  let* name = check_name "`operation`" name_span in
  let* parameters, rest =
    if starts "(" rest then read_delimited 0 '(' ')' rest
    else Ok (slice rest.start rest.start rest, rest)
  in
  let* params, _ = comma_items None read_param_prefix parameters in
  let rest = horizontal rest in
  if not (starts "->" rest) then at rest (Missing_separator ("->", text name_span))
  else
    let results = horizontal (drop 2 rest) in
    let* outputs, rest =
      if starts "(" results then
        let* inside, rest = read_delimited 0 '(' ')' results in
        let+ outputs, _ = comma_items None (read_output_prefix ~stops:[ "," ]) inside in
        (outputs, rest)
      else
        let bounded =
          if starts "\n" results || starts "\r" results then
            slice results.start results.start results
          else results
        in
        let+ output, rest = read_output_prefix ~stops:[ ","; ":"; "\r"; "\n" ] bounded in
        ([ output ], if empty bounded then results else rest)
    in
    Ok
      ( { span = trim (slice input.start rest.start input); name; params; outputs; body = None },
        rest )

let read_operation input =
  let* operation, rest = read_operation_prefix input in
  let+ () = require_end rest in
  operation

let read_iteration span =
  let* name, args = read_annotation (fun s -> Invalid_argument_annotation s) span in
  let dimensions constructor dims =
    if dims = [] then at span (Invalid_argument_annotation (text span))
    else
      let+ dims = read_list (check_name "a dimension") dims in
      constructor dims
  in
  match (name, args) with
  | "vary", dims -> dimensions (fun d -> Vary d) dims
  | "same", dims -> dimensions (fun d -> Same d) dims
  | "each", dims -> dimensions (fun dims -> Each dims) dims
  | "first", [ dim ] ->
      let+ dim = check_name "a dimension" dim in
      First dim
  | "last", [ dim ] ->
      let+ dim = check_name "a dimension" dim in
      Last dim
  | "where", (_ :: _ as pairs) ->
      let pair input =
        let* key, value = separated "=" input in
        let* key = check_name "a dimension" key in
        if empty value then at value Empty_value else Ok (key, text value)
      in
      let+ pairs = read_list pair pairs in
      Where pairs
  | _ -> at span (Invalid_argument_annotation (text span))

let iteration_name = function
  | Vary _ -> "vary"
  | Each _ -> "each"
  | Same _ -> "same"
  | Where _ -> "where"
  | First _ -> "first"
  | Last _ -> "last"

let read_arg span =
  let* tokens = words span in
  match tokens with
  | value :: annotations ->
      let* artifact = read_ref value in
      let* iterations =
        read_list
          (fun span ->
            let+ value = read_iteration span in
            { span; value })
          annotations
      in
      let rec validate seen = function
        | [] -> Ok ()
        | (iteration : iteration located) :: rest ->
            let name = iteration_name iteration.value in
            let selection = function "vary" | "same" | "first" | "last" -> true | _ -> false in
            if List.mem name seen || (selection name && List.exists selection seen) then
              at span (Repeated_iteration name)
            else validate (name :: seen) rest
      in
      let+ () = validate [] iterations in
      { span; artifact_span = value; artifact; iterations }
  | [] -> at span (Invalid_argument (text span))

let read_type_parameter input =
  let* value, rest = read_type_prefix 0 input in
  match value with
  | Var name -> Ok (name, rest)
  | _ -> at (trim (slice input.start rest.start input)) Type_parameter_needs_variable

let read_type_def_prefix input =
  if starts "$" input then at input (Defined_type_variable (text (drop 1 input)))
  else
    let* name_span, rest = name_prefix input in
    let* name = type_name name_span in
    let* params, rest =
      if starts "<" rest then comma_items (Some ">") read_type_parameter (drop 1 rest)
      else Ok ([], rest)
    in
    Ok (({ span = trim (slice input.start rest.start input); name; params } : type_def), rest)

let read_type_def input =
  let* definition, rest = read_type_def_prefix input in
  let+ () = require_end rest in
  definition

let binding_description description span =
  let+ dims =
    match (description.shape, description.placement, description.gates) with
    | File None, None, [] -> Ok None
    | File None, Some (Dims dims), [] -> Ok (Some dims)
    | _ -> at span (Binding_description (text span))
  in
  { typ = description.typ; dims }

let read_product_prefix input =
  let* names, rest = read_until 0 [ ":"; "="; "\r"; "\n" ] input in
  let* name_spans = comma_list names in
  let* names = read_list (check_name "`product`") name_spans in
  let* declared, rest =
    if starts ":" rest then
      let* description, rest =
        read_description_prefix ~stops:[ "="; "\r"; "\n" ] (horizontal (drop 1 rest))
      in
      let+ declared = binding_description description input in
      (Some declared, rest)
    else Ok (None, rest)
  in
  if not (starts "=" rest) then at rest (Missing_separator ("=", text input))
  else
    let call_start = horizontal (drop 1 rest) in
    let* callee_span, opening = read_until 0 [ "("; "\r"; "\n" ] call_start in
    if not (starts "(" opening) then at callee_span Explicit_call_required
    else
      let* callee = read_ref callee_span in
      let* inside, rest = read_delimited 0 '(' ')' opening in
      let* arguments = comma_list inside in
      let* args = read_list read_arg arguments in
      let call = { span = slice call_start.start rest.start input; callee_span; callee; args } in
      if names = [] then at input (Missing_name "`product`")
      else
        Ok ({ span = slice input.start rest.start input; name_spans; names; call; declared }, rest)

let source name_span name description_span description =
  match description.shape with
  | File _ | Folder ->
      let found =
        {
          typ = description.typ;
          shape = description.shape;
          placement = Option.value description.placement ~default:(Dims []);
        }
      in
      Ok { value = { span = name_span; name; origin = Found found }; gates = description.gates }
  | _ -> at description_span (Folder_source name)

let read_source_prefix input =
  let* name_span, rest = read_until 0 [ ":"; "\r"; "\n" ] input in
  let* name = check_name "`source`" name_span in
  let value =
    if starts ":" rest then horizontal (drop 1 rest) else slice rest.start rest.start rest
  in
  if starts ":" rest && (empty value || starts "\r" value || starts "\n" value) then
    Ok (Source_body_header (name_span, name), rest)
  else
    let* description, remaining = read_description_prefix ~stops:[ "\r"; "\n" ] value in
    let+ parsed = source name_span name value description in
    (Source_header parsed, if empty value then rest else remaining)
