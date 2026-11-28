open Ast
open Data

type names = {
  operation : string -> string;
  artifact : string -> string;
  gate : string -> string;
  typ : string -> string;
}

let identity = { operation = Fun.id; artifact = Fun.id; gate = Fun.id; typ = Fun.id }

let reference rename reference = { namespace = None; name = rename (ref_name reference) }

let rec typ names = function
  | Var _ as value -> value
  | Named (name, args) -> Named (reference names.typ name, List.map (typ names) args)

let declared names (value : declared) = { value with typ = Option.map (typ names) value.typ }

let call names ~local (value : call) =
  let args =
    List.map
      (fun (arg : arg) ->
        {
          arg with
          artifact = (if local then arg.artifact else reference names.artifact arg.artifact);
        })
      value.args
  in
  { value with callee = reference names.operation value.callee; args }

let binding names (value : binding) =
  {
    value with
    call = call names ~local:true value.call;
    declared = Option.map (declared names) value.declared;
  }

let operation names (value : operation_decl) =
  {
    value with
    name = names.operation value.name;
    params =
      List.map (fun (p : param) -> { p with typ = Option.map (typ names) p.typ }) value.params;
    outputs =
      List.map (fun (o : output) -> { o with typ = Option.map (typ names) o.typ }) value.outputs;
    body = Option.map (List.map (binding names)) value.body;
  }

let artifact names (value : Ast.artifact) =
  let origin =
    match value.origin with
    | Found found ->
        let placement = match found.placement with other -> other in
        Found { found with typ = Option.map (typ names) found.typ; placement }
    | Made (job, output, d) -> Made (job, output, Option.map (declared names) d)
  in
  { value with name = names.artifact value.name; origin }

let operation_target names = function
  | Every_operation -> Every_operation
  | Named_operation name -> Named_operation (names.operation name)

let artifact_target names = function
  | Named_artifact name -> Named_artifact (names.artifact name)
  | Input_of (op, port) -> Input_of (names.operation op, port)
  | Output_of (op, port) -> Output_of (names.operation op, port)
  | other -> other

let setting names (value : Setting.t) =
  match value with
  | Command (target, script) -> Setting.Command (operation_target names target, script)
  | Verify (target, script) -> Verify (operation_target names target, script)
  | Calculate (target, text) -> Calculate (operation_target names target, text)
  | Resources (target, values) -> Resources (operation_target names target, values)
  | Uses (target, uses) -> Uses (operation_target names target, uses)
  | Anchored target -> Anchored (operation_target names target)
  | Path (target, text) -> Path (artifact_target names target, text)
  | Ext (target, ext) -> Ext (artifact_target names target, ext)
  | Gate (kind, target, uses) ->
      Gate
        ( kind,
          artifact_target names target,
          List.map (fun (use : gate_use) -> { use with gate = reference names.gate use.gate }) uses
        )
  | Entities _ as value -> value

let gate names = function
  | Check test -> Check { test with name = names.gate test.name }
  | Screen test -> Screen { test with name = names.gate test.name }
  | Review name -> Review (names.gate name)

let rec scope names value =
  {
    operations = List.map (operation names) value.operations;
    artifacts = List.map (artifact names) value.artifacts;
    jobs = List.map (call names ~local:false) value.jobs;
    settings =
      List.map
        (fun (s : Setting.t located) -> { s with value = setting names s.value })
        value.settings;
    stages = List.map (fun (s : stage) -> { s with scope = scope names s.scope }) value.stages;
  }
