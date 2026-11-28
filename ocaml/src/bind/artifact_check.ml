open Ast
open Data

type bound = {
  when_ : string;
  name : string;
  port : string;
  path : string;
  command : Json.t list list;
}

let json check =
  Json.Object
    [
      ("when", Json.Text check.when_);
      ("check", Json.Text check.name);
      ("port", Json.Text check.port);
      ("path", Json.Text check.path);
      ("command", Json.list (fun parts -> Json.List parts) check.command);
    ]

let definition ?(kind = Gate_kind.Check) gates name =
  match
    List.find_map
      (fun gate ->
        match (kind, gate) with
        | (Gate_kind.Check, Check test | Gate_kind.Screen, Screen test) when test.name = name ->
            Some test
        | _ -> None)
      gates
  with
  | Some test -> Ok test
  | None when kind = Gate_kind.Screen -> error (Problem.Screen_unknown name)
  | None -> error (Problem.Check_unknown name)

let arguments (test : test) (used : gate_use) =
  if List.length test.params <> List.length used.args then error (Problem.Check_arguments test.name)
  else Ok (List.combine test.params used.args)

let part path values = function
  | Ast.Text text -> Ok (Json.Text text.value)
  | Expand { value = Builtin "path"; _ } -> Ok (Json.Object [ ("path", Json.Text path) ])
  | Expand { value = Port name; _ } -> (
      match List.assoc_opt name values with
      | Some value -> Ok (Json.Text value)
      | None -> error (Problem.Check_parameter_unknown name))
  | Expand expansion -> error ~span:expansion.span Problem.Check_placeholder

let command path (test : test) used =
  let* values = arguments test used in
  match test.script.commands with
  | [ command ] ->
      let* words =
        read_list
          (fun (word : command_word) ->
            if word.role <> Argument then error (Problem.Check_redirect test.name)
            else read_list (part path values) word.parts)
          command.words
      in
      if words = [] then error (Problem.Check_no_executable test.name) else Ok words
  | _ -> error (Problem.Check_one_command test.name)

let label (used : gate_use) =
  let name = ref_name used.gate in
  match used.args with [] -> name | args -> name ^ "(" ^ String.concat ", " args ^ ")"

let uses ?(kind = Gate_kind.Check) settings applies =
  let add found used =
    let same other = other.gate = used.gate && other.args = used.args in
    if used.negated then List.filter (fun other -> not (same other)) found
    else if List.exists same found then found
    else found @ [ used ]
  in
  List.fold_left
    (fun found (setting : Setting.t located) ->
      match setting.value with
      | Gate (found_kind, target, uses) when found_kind = kind && applies target ->
          List.fold_left add found uses
      | _ -> found)
    [] settings

let bind_target ?(kind = Gate_kind.Check) resolved paths settings when_ port id applies =
  let uses = uses ~kind settings applies in
  read_list
    (fun used ->
      let* test = definition ~kind resolved.checked.pipeline.syntax.gates (ref_name used.gate) in
      let path = paths.(Artifact_id.index id) in
      let+ command = command path test used in
      { when_; name = label used; port; path; command })
    uses

let bind resolved paths (job : job) =
  let step = resolved.checked.steps.(job.step) in
  let operation = step.operation in
  let* inputs =
    read_list
      (fun ((param : param), ids) ->
        let* checks =
          read_list
            (fun id ->
              let product = product resolved.checked (artifact resolved id).product in
              bind_target resolved paths (product.settings @ step.settings) "before" param.name id
                (function
                | Named_artifact name -> name = product.name
                | Input_of (name, port) -> name = operation.name && port = param.name
                | _ -> false))
            (Array.to_list ids)
        in
        Ok (List.concat checks))
      (List.combine operation.params (Array.to_list job.inputs))
  in
  let+ outputs =
    read_list
      (fun (output, id) ->
        let product = product resolved.checked (artifact resolved id).product in
        bind_target resolved paths product.settings "after" (port_name output) id (function
          | Every_artifact -> true
          | Named_artifact name -> name = product.name
          | Output_of (name, port) -> name = operation.name && port = output.name
          | Extension extension -> String.ends_with ~suffix:extension paths.(Artifact_id.index id)
          | _ -> false))
      (List.combine operation.outputs (Array.to_list job.outputs))
  in
  List.concat (inputs @ outputs)

let screen_json (check, product_scope) =
  Json.Object
    [
      ("when", Json.Text check.when_);
      ("screen", Json.Text check.name);
      ("port", Json.Text check.port);
      ("path", Json.Text check.path);
      ("product_scope", Json.Bool product_scope);
      ("command", Json.list (fun parts -> Json.List parts) check.command);
    ]

let screens resolved paths (job : job) =
  let kind = Gate_kind.Screen in
  let step = resolved.checked.steps.(job.step) in
  let operation = step.operation in
  let scoped product_scope = List.map (fun check -> (check, product_scope)) in
  let* inputs =
    read_list
      (fun ((param : param), ids) ->
        let+ found =
          read_list
            (fun id ->
              let product = product resolved.checked (artifact resolved id).product in
              let* product_wide =
                bind_target ~kind resolved paths product.settings "before" param.name id (function
                  | Named_artifact name -> name = product.name
                  | _ -> false)
              in
              let+ port =
                bind_target ~kind resolved paths step.settings "before" param.name id (function
                  | Input_of (name, port) -> name = operation.name && port = param.name
                  | _ -> false)
              in
              scoped true product_wide @ scoped false port)
            (Array.to_list ids)
        in
        List.concat found)
      (List.combine operation.params (Array.to_list job.inputs))
  in
  let+ outputs =
    read_list
      (fun (output, id) ->
        let product = product resolved.checked (artifact resolved id).product in
        let+ found =
          bind_target ~kind resolved paths product.settings "after" (port_name output) id (function
            | Every_artifact -> true
            | Named_artifact name -> name = product.name
            | Output_of (name, port) -> name = operation.name && port = output.name
            | _ -> false)
        in
        scoped true found)
      (List.combine operation.outputs (Array.to_list job.outputs))
  in
  List.concat (inputs @ outputs)
