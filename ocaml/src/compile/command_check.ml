open Ast
open Data

type context = {
  verify : bool;
  params : param array;
  outputs : output array;
  input_dims : Symbol.t array array;
  output_dims : Symbol.t array;
  symbols : symbols;
}

let input context name = Array.find_index (fun (p : param) -> p.name = name) context.params

let output context name = Array.find_index (fun (o : output) -> port_name o = name) context.outputs

let unnamed context =
  match Array.to_list context.outputs with [ { name = None; _ } ] -> true | _ -> false

let verify_access context expansion =
  if context.verify then
    match expansion with
    | Builtin "output" | Property _ -> error "verify" "verify commands may name only input ports"
    | Port name when input context name = None && output context name <> None ->
        error "verify" "verify commands may name only input ports"
    | _ -> Ok ()
  else Ok ()

let property context name property =
  let outputs = context.outputs in
  let name = if name = "@output" then "output" else name in
  match output context name with
  | None -> error name "path properties belong to output ports"
  | Some index -> (
      if property = "dir" then Ok ()
      else
        match outputs.(index).shape with
        | File (Some (Replace _)) -> Ok ()
        | _ -> error name "stem and prefix require a declared extension")

let check_expansion context expansion =
  let* () = verify_access context expansion in
  match expansion with
  | Port name ->
      if
        input context name <> None
        || output context name <> None
        || Array.exists (fun d -> symbol_text context.symbols d = name) context.output_dims
      then Ok ()
      else error name "placeholder names no port or job dimension"
  | Builtin "output" ->
      if unnamed context then Ok () else error "@output" "use the declared output context name"
  | Builtin ("cores" | "here") -> Ok ()
  | Builtin name -> error name "unsupported command built-in"
  | Property (name, kind) -> property context name kind
  | Dimension (name, dim) -> (
      match input context name with
      | Some index
        when Array.exists (fun d -> symbol_text context.symbols d = dim) context.input_dims.(index)
        ->
          Ok ()
      | _ -> error name "input lacks the projected dimension")
  | Function (("join" | "list"), [ name ]) -> (
      match input context name with
      | Some index when context.params.(index).many -> Ok ()
      | _ -> error name "join and list require a many input")
  | Function ("scalar", name :: options) -> (
      match input context name with
      | Some index when not context.params.(index).many ->
          let+ _ = Scalar.options options in
          ()
      | _ -> error name "scalar requires one input file")
  | Function _ -> error "command" "unknown command function"

let is_many context = function
  | Port name | Dimension (name, _) -> (
      match input context name with Some i -> context.params.(i).many | _ -> false)
  | _ -> false

let validate_word context word =
  let* _ =
    read_list (function Text _ -> Ok () | Expand e -> check_expansion context e.value) word.parts
  in
  if
    List.exists (function Expand e -> is_many context e.value | _ -> false) word.parts
    && (word.quoted || List.length word.parts <> 1)
  then error "command" "a many placeholder must fill an unquoted whole argument"
  else Ok ()

let validate_command context (command : command) =
  let* () =
    if context.verify && List.exists (fun word -> word.role <> Argument) command.words then
      error "verify" "verify commands cannot redirect streams"
    else Ok ()
  in
  let+ _ = read_list (validate_word context) command.words in
  ()

let validate ~verify (operation : operation_decl) input_dims output_dims symbols lines =
  let context =
    {
      verify;
      params = Array.of_list operation.params;
      outputs = Array.of_list operation.outputs;
      input_dims;
      output_dims;
      symbols;
    }
  in
  let+ _ = read_list (validate_command context) lines in
  ()
