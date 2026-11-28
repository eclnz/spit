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
    | Builtin "output" | Property _ -> error Problem.Verify_names_inputs
    | Port name when input context name = None && output context name <> None ->
        error Problem.Verify_names_inputs
    | _ -> Ok ()
  else Ok ()

let property context name property =
  let outputs = context.outputs in
  let name = if name = "@output" then "output" else name in
  match output context name with
  | None -> error (Problem.Property_on_input name)
  | Some index -> (
      if property = "dir" then Ok ()
      else
        match outputs.(index).shape with
        | File (Some (Replace _)) -> Ok ()
        | _ -> error (Problem.Stem_needs_extension name))

let check_expansion context expansion =
  let* () = verify_access context expansion in
  match expansion with
  | Port name ->
      if
        input context name <> None
        || output context name <> None
        || Array.exists (fun d -> symbol_text context.symbols d = name) context.output_dims
      then Ok ()
      else error (Problem.Placeholder_unknown name)
  | Builtin "output" -> if unnamed context then Ok () else error Problem.Output_context
  | Builtin ("cores" | "here") -> Ok ()
  | Builtin name -> error (Problem.Builtin_unknown name)
  | Property (name, kind) -> property context name kind
  | Dimension (name, dim) -> (
      match input context name with
      | Some index
        when Array.exists (fun d -> symbol_text context.symbols d = dim) context.input_dims.(index)
        ->
          Ok ()
      | _ -> error (Problem.Projection_missing name))
  | Function (("join" | "list"), [ name ]) -> (
      match input context name with
      | Some index when context.params.(index).many -> Ok ()
      | _ -> error (Problem.Collection_function name))
  | Function ("scalar", name :: options) -> (
      match input context name with
      | Some index when not context.params.(index).many ->
          let+ _ = Scalar.options options in
          ()
      | _ -> error (Problem.Scalar_input name))
  | Function _ -> error Problem.Function_unknown

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
  then error Problem.Many_whole_argument
  else Ok ()

let validate_command context (command : command) =
  let* () =
    if context.verify && List.exists (fun word -> word.role <> Argument) command.words then
      error Problem.Verify_redirect
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
