open Ast
open Data
open Json

type parser = {
  operation : operation_decl;
  input : span;
  mutable cursor : int;
  mutable nodes : int;
}

let failure state message =
  error
    ~span:
      { state.input with start = state.cursor; finish = min state.input.finish (state.cursor + 1) }
    (Problem.Calculation { operation = state.operation.name; message })

let whitespace state =
  while state.cursor < state.input.finish && Span.whitespace state.input.source.[state.cursor] do
    state.cursor <- state.cursor + 1
  done

let take state c =
  whitespace state;
  if state.cursor < state.input.finish && state.input.source.[state.cursor] = c then (
    state.cursor <- state.cursor + 1;
    true)
  else false

let node state value =
  state.nodes <- state.nodes + 1;
  if state.nodes > 128 then failure state "a calculation has at most 128 nodes" else Ok value

let digits state =
  let start = state.cursor in
  while
    state.cursor < state.input.finish
    && state.input.source.[state.cursor] >= '0'
    && state.input.source.[state.cursor] <= '9'
  do
    state.cursor <- state.cursor + 1
  done;
  state.cursor - start

let exponent state =
  let source = state.input.source and finish = state.input.finish in
  if state.cursor < finish && (source.[state.cursor] = 'e' || source.[state.cursor] = 'E') then (
    state.cursor <- state.cursor + 1;
    if state.cursor < finish && (source.[state.cursor] = '+' || source.[state.cursor] = '-') then
      state.cursor <- state.cursor + 1;
    if digits state = 0 then failure state "an exponent needs digits" else Ok ())
  else Ok ()

let number state =
  let start = state.cursor in
  let whole = digits state in
  let fraction =
    if state.cursor < state.input.finish && state.input.source.[state.cursor] = '.' then (
      state.cursor <- state.cursor + 1;
      digits state)
    else 0
  in
  if whole + fraction = 0 then
    failure state "expected a number, input port or calculation expression"
  else
    let* () = exponent state in
    match float_of_string_opt (String.sub state.input.source start (state.cursor - start)) with
    | Some value when Float.is_finite value -> node state (Float value)
    | _ -> failure state "calculation numbers must be finite"

let operator state =
  if state.cursor = state.input.finish then None
  else
    match state.input.source.[state.cursor] with
    | '+' -> Some ("+", 10)
    | '-' -> Some ("-", 10)
    | '*' -> Some ("*", 20)
    | '/' -> Some ("/", 20)
    | '^' -> Some ("^", 40)
    | _ -> None

let function_call state name args =
  let arity =
    match name with
    | "exp" | "log" | "sqrt" | "abs" -> Some 1
    | "min" | "max" | "round" -> Some 2
    | _ -> None
  in
  if arity <> Some (List.length args) then
    failure state "unknown calculation function or wrong argument count"
  else node state (List (Text name :: args))

let rec expression state depth minimum =
  if depth > 64 then failure state "a calculation nests at most 64 levels"
  else
    let* left = prefix state depth in
    infix state depth minimum left

and infix state depth minimum left =
  whitespace state;
  match operator state with
  | Some (op, precedence) when precedence >= minimum ->
      state.cursor <- state.cursor + 1;
      let* right = expression state (depth + 1) (if op = "^" then precedence else precedence + 1) in
      let* combined = node state (List [ Text op; left; right ]) in
      infix state depth minimum combined
  | _ -> Ok left

and arguments state depth acc =
  if take state ')' then Ok (List.rev acc)
  else
    let* value = expression state (depth + 1) 0 in
    if take state ',' then arguments state depth (value :: acc)
    else if take state ')' then Ok (List.rev (value :: acc))
    else failure state "expected comma or closing calculation parenthesis"

and named_term state depth =
  let start = state.cursor in
  while state.cursor < state.input.finish && Span.name_char state.input.source.[state.cursor] do
    state.cursor <- state.cursor + 1
  done;
  let name = String.sub state.input.source start (state.cursor - start) in
  if take state '(' then
    let* args = arguments state depth [] in
    function_call state name args
  else if List.exists (fun (p : param) -> p.name = name && not p.many) state.operation.params then
    node state (Text name)
  else failure state "a calculation name must be a single-file input port"

and prefix state depth =
  whitespace state;
  if take state '+' then expression state (depth + 1) 30
  else if take state '-' then
    let* value = expression state (depth + 1) 30 in
    node state (List [ Text "neg"; value ])
  else if take state '(' then
    let* value = expression state (depth + 1) 0 in
    if take state ')' then Ok value else failure state "expected closing calculation parenthesis"
  else if
    state.cursor < state.input.finish
    && (Span.letter state.input.source.[state.cursor] || state.input.source.[state.cursor] = '_')
  then named_term state depth
  else number state

let parse operation (input : string located) =
  let state = { operation; input = input.span; cursor = input.span.start; nodes = 0 } in
  let* value = expression state 0 0 in
  whitespace state;
  if state.cursor <> state.input.finish then failure state "unexpected calculation text"
  else Ok value
