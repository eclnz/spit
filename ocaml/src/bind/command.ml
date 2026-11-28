open Ast
open Data

type part = Ast.command_part = Text of string located | Expand of expansion located

type context = {
  resolved : resolved;
  paths : string array;
  job : job;
  step : step;
  params : param array;
  outputs : output array;
}

let path_part paths id = Json.Object [ ("path", Json.Text paths.(Artifact_id.index id)) ]

let find_output context name =
  let outputs = context.outputs and job = context.job in
  let index = Array.find_index (fun output -> port_name output = name) outputs in
  match index with
  | None -> error name "unknown output port"
  | Some index -> Ok (job.outputs.(index), outputs.(index))

let input context name =
  let params = context.params and job = context.job in
  match Array.find_index (fun (p : param) -> p.name = name) params with
  | None -> error name "unknown input port"
  | Some index -> Ok (index, job.inputs.(index))

let member port offset value =
  let open Json in
  Object [ ("port", Number port); ("offset", Number offset); ("member", value) ]

let port context name =
  let params = context.params
  and job = context.job
  and paths = context.paths
  and resolved = context.resolved in
  let open Json in
  match Array.find_index (fun (p : param) -> p.name = name) params with
  | Some index ->
      let values =
        Array.mapi
          (fun offset id ->
            let value = path_part paths id in
            if params.(index).many then [ member index offset value ] else [ value ])
          job.inputs.(index)
      in
      Ok (Array.to_list values, params.(index).many)
  | None -> (
      match Array.find_index (fun o -> port_name o = name) context.outputs with
      | Some index -> Ok ([ [ path_part paths job.outputs.(index) ] ], false)
      | None -> (
          match Hashtbl.find_opt resolved.checked.symbols.ids name with
          | Some dim -> (
              match Array.find_opt (fun (d, _) -> dim = d) job.entities with
              | Some (_, v) -> Ok ([ [ Text (symbol_text resolved.checked.symbols v) ] ], false)
              | None -> error name "job does not have this dimension")
          | None -> error name "placeholder names no port or dimension"))

let here context =
  let resolved = context.resolved and step = context.step in
  let open Json in
  let files = resolved.checked.pipeline.files in
  let root = match files with (path, _) :: _ -> Filename.dirname path | [] -> "" in
  let file = List.find_opt (fun (_, source) -> source == step.operation.span.source) files in
  let directory = Option.fold ~none:root ~some:(fun (path, _) -> Filename.dirname path) file in
  if directory = root then Ok ([ [ Object [ ("here", Text "") ] ] ], false)
  else if String.starts_with ~prefix:(root ^ "/") directory then
    let relative =
      String.sub directory
        (String.length root + 1)
        (String.length directory - String.length root - 1)
    in
    Ok ([ [ Object [ ("here", Text relative) ] ] ], false)
  else error "@here" "an imported command's script folder lies outside the pipeline folder"

let property context name property =
  let paths = context.paths in
  let open Json in
  let* id, output = find_output context (if name = "@output" then "output" else name) in
  let path = paths.(Artifact_id.index id) in
  let* value =
    match property with
    | "dir" -> Ok (Filename.dirname path)
    | "stem" | "prefix" -> (
        let ext = match output.shape with File (Some (Replace ext)) -> Some ext | _ -> None in
        match ext with
        | None -> error name "stem and prefix require a declared extension"
        | Some ext when String.ends_with ~suffix:ext path ->
            let prefix = String.sub path 0 (String.length path - String.length ext) in
            Ok (if property = "stem" then Filename.basename prefix else prefix)
        | Some _ -> error name "output path differs from its declared extension")
    | _ -> error name "unknown output property"
  in
  let parts =
    if property = "prefix" then
      [
        Object [ ("dir", Text (Filename.dirname value)); ("of", Text path) ];
        Text "/";
        Object [ ("stem", Text (Filename.basename value)); ("of", Text path) ];
      ]
    else [ Object [ (property, Text value); ("of", Text path) ] ]
  in
  Ok ([ parts ], false)

let dimension context name dimension =
  let resolved = context.resolved and params = context.params in
  let open Json in
  let* index, ids = input context name in
  let* values =
    read_list
      (fun (offset, id) ->
        let artifact = Data.artifact resolved id in
        let dim = intern resolved.checked.symbols dimension in
        match Array.find_opt (fun (d, _) -> d = dim) artifact.entities with
        | None -> error name "input lacks the projected dimension"
        | Some (_, v) ->
            let value = Text (symbol_text resolved.checked.symbols v) in
            Ok [ (if params.(index).many then member index offset value else value) ])
      (List.mapi (fun i id -> (i, id)) (Array.to_list ids))
  in
  Ok (values, params.(index).many)

let collection context name port =
  let paths = context.paths and params = context.params in
  let open Json in
  let* index, ids = input context port in
  if not params.(index).many then error port "join and list require a many input"
  else
    let paths =
      List (Array.to_list (Array.map (fun id -> Text paths.(Artifact_id.index id)) ids))
    in
    let fields = [ (name, paths); ("port", Number index) ] in
    Ok
      ( [ [ Object (if name = "join" then fields @ [ ("separator", Text ",") ] else fields) ] ],
        false )

let scalar context name options =
  let paths = context.paths and params = context.params in
  let open Json in
  let* index, ids = input context name in
  if params.(index).many || Array.length ids <> 1 then error name "scalar requires one input file"
  else
    let+ bounds = Scalar.options options in
    ( [
        [
          Object
            [
              ("scalar", Text paths.(Artifact_id.index ids.(0)));
              ("min", option (fun n -> Float n) bounds.minimum);
              ("max", option (fun n -> Float n) bounds.maximum);
            ];
        ];
      ],
      false )

let expand context = function
  | Port name -> port context name
  | Builtin "output" -> (
      match context.step.operation.outputs with
      | [ { name = None; _ } ] -> Ok ([ [ path_part context.paths context.job.outputs.(0) ] ], false)
      | _ -> error "@output" "use the declared output port name")
  | Builtin "cores" -> Ok ([ [ Json.Text (string_of_int context.step.configuration.cores) ] ], false)
  | Builtin "here" -> here context
  | Property (name, kind) -> property context name kind
  | Dimension (name, dim) -> dimension context name dim
  | Function ((("join" | "list") as name), [ port ]) -> collection context name port
  | Function ("scalar", name :: options) -> scalar context name options
  | Function _ -> error "command" "unknown command function"
  | Builtin name -> error name "unsupported command built-in"

let bind_word context word =
  let open Json in
  match word.parts with
  | [ Expand expansion ] ->
      let* values, many = expand context expansion.value in
      if many && word.quoted then
        error "command" "a many placeholder must be an unquoted whole argument"
      else Ok values
  | parts ->
      let* values =
        read_list
          (fun (part : part) ->
            match part with
            | Text value -> Ok [ Text value.value ]
            | Expand expansion ->
                let* values, many = expand context expansion.value in
                if many || List.length values <> 1 then
                  error "command" "a many placeholder must fill a whole argument"
                else Ok (List.hd values))
          parts
      in
      Ok [ List.concat values ]

let bind_line ~redirect context (command : command) =
  let paths = context.paths and job = context.job in
  let words = command.words in
  if words = [] then Ok None
  else
    let rec read arguments stdin stdout = function
      | [] -> Ok (List.rev arguments, stdin, stdout)
      | word :: rest when word.role <> Argument -> (
          if not redirect then error "command" "verify commands cannot redirect streams"
          else
            match rest with
            | target :: rest when target.role = Argument -> (
                let* values = bind_word context target in
                match values with
                | [ [ Json.Object [ ("path", Json.Text path) ] ] ] ->
                    let belongs ids =
                      Array.exists (fun id -> paths.(Artifact_id.index id) = path) ids
                    in
                    if word.role = Output_redirect && stdout = None && belongs job.outputs then
                      read arguments stdin (Some path) rest
                    else if
                      word.role = Input_redirect && stdin = None && Array.exists belongs job.inputs
                    then read arguments (Some path) stdout rest
                    else error "command" "redirection must name an appropriate port, once"
                | _ -> error "command" "redirection needs one artifact path")
            | _ -> error "command" "redirection needs a following port")
      | word :: rest ->
          let* values = bind_word context word in
          read (List.rev_append values arguments) stdin stdout rest
    in
    let* arguments, stdin, stdout = read [] None None words in
    match arguments with
    | [] -> error "command" "command has no executable"
    | _ -> Ok (Some (arguments, stdin, stdout))

let bind ~redirect resolved paths job (step : step) lines =
  let context =
    {
      resolved;
      paths;
      job;
      step;
      params = Array.of_list step.operation.params;
      outputs = Array.of_list step.operation.outputs;
    }
  in
  let+ commands = read_list (bind_line ~redirect context) lines in
  List.filter_map Fun.id commands
