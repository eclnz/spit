open Ast
open Data

let operation_index program =
  let pipeline = program.syntax in
  let operations = Hashtbl.create 32 in
  let pending = Stack.create () in
  List.iter (fun scope -> Stack.push scope pending) (List.rev program.libraries);
  Stack.push pipeline.global pending;
  while not (Stack.is_empty pending) do
    let scope = Stack.pop pending in
    List.iter (fun (o : operation_decl) -> Hashtbl.replace operations o.name o) scope.operations;
    List.iter (fun (s : stage) -> Stack.push s.scope pending) scope.stages
  done;
  operations

let validate (operation : operation_decl) =
  match operation.body with
  | None -> Ok ()
  | Some body ->
      let locals = Hashtbl.create 16 in
      List.iter (fun (p : param) -> Hashtbl.add locals p.name ()) operation.params;
      let* _ =
        read_list
          (fun (binding : binding) ->
            let* _ =
              read_list
                (fun (arg : arg) ->
                  if Hashtbl.mem locals (ref_name arg.artifact) then Ok ()
                  else error ~span:arg.artifact_span (Problem.Composite_input operation.name))
                binding.call.args
            in
            let+ _ =
              read_list
                (fun (name, span) ->
                  if Hashtbl.mem locals name then
                    error ~span (Problem.Composite_assigned_twice name)
                  else (
                    Hashtbl.add locals name ();
                    Ok ()))
                (List.combine binding.names binding.name_spans)
            in
            ())
          body
      in
      let+ _ =
        read_list
          (fun (output : output) ->
            match output.name with
            | Some name
              when Hashtbl.mem locals name
                   && not (List.exists (fun (p : param) -> p.name = name) operation.params) ->
                Ok ()
            | _ -> error ~span:output.span (Problem.Composite_unassigned operation.name))
          operation.outputs
      in
      ()

let check_cycles (operations : (string, operation_decl) Hashtbl.t) =
  let colors = Hashtbl.create 16 in
  let rec cycle tasks =
    match tasks with
    | [] -> Ok ()
    | (name, finish) :: rest -> (
        if finish then (
          Hashtbl.replace colors name 2;
          cycle rest)
        else
          match Hashtbl.find_opt colors name with
          | Some 1 -> error (Problem.Composite_cycle name)
          | Some 2 -> cycle rest
          | _ ->
              Hashtbl.replace colors name 1;
              let children =
                match Hashtbl.find_opt operations name with
                | Some { body = Some body; _ } ->
                    List.map (fun binding -> (ref_name binding.call.callee, false)) body
                | _ -> []
              in
              cycle (children @ ((name, true) :: rest)))
  in
  cycle (Hashtbl.fold (fun name _ acc -> (name, false) :: acc) operations [])

type environment = {
  inputs : (string * arg) list;
  outputs : (string * (string * span)) list;
  prefix : string;
}

let local_name environment name span =
  match List.assoc_opt name environment.outputs with
  | Some named -> named
  | None -> (environment.prefix ^ "::" ^ name, span)

let rewrite_arg environment (arg : arg) =
  match List.assoc_opt (ref_name arg.artifact) environment.inputs with
  | Some input ->
      {
        arg with
        artifact = input.artifact;
        artifact_span = input.artifact_span;
        iterations = input.iterations @ arg.iterations;
      }
  | None ->
      let name, _ = local_name environment (ref_name arg.artifact) arg.artifact_span in
      { arg with artifact = { namespace = None; name } }

let rewrite_binding environment (local : binding) =
  let names = List.map2 (local_name environment) local.names local.name_spans in
  {
    local with
    names = List.map fst names;
    name_spans = List.map snd names;
    call = { local.call with args = List.map (rewrite_arg environment) local.call.args };
  }

let expansion_environment (operation : operation_decl) (binding : binding) =
  let callee = ref_name binding.call.callee in
  if List.length operation.params <> List.length binding.call.args then
    error ~span:binding.call.span (Problem.Composite_argument_count callee)
  else if List.length operation.outputs <> List.length binding.names then
    error ~span:binding.span (Problem.Composite_output_count callee)
  else
    let inputs =
      List.map2 (fun (p : param) arg -> (p.name, arg)) operation.params binding.call.args
    in
    let outputs =
      List.map2
        (fun (output : output) named -> (Option.get output.name, named))
        operation.outputs
        (List.combine binding.names binding.name_spans)
    in
    Ok { inputs; outputs; prefix = List.hd binding.names }

let expand_binding (operations : (string, operation_decl) Hashtbl.t) contracts (binding : binding) =
  let work = Stack.create () and result = ref [] in
  Stack.push binding work;
  let rec loop () =
    if Stack.is_empty work then Ok (List.rev !result)
    else
      let binding = Stack.pop work in
      match Hashtbl.find_opt operations (ref_name binding.call.callee) with
      | Some ({ body = Some body; _ } as operation) ->
          let* environment = expansion_environment operation binding in
          contracts :=
            {
              span = binding.span;
              operation;
              args = binding.call.args;
              outputs = binding.names;
              declared = binding.declared;
            }
            :: !contracts;
          List.iter
            (fun local -> Stack.push (rewrite_binding environment local) work)
            (List.rev body);
          loop ()
      | _ ->
          result := binding :: !result;
          loop ()
  in
  loop ()

let scope_bindings (value : scope) =
  let grouped = Array.make (List.length value.jobs) [] in
  List.iter
    (fun (artifact : Ast.artifact) ->
      match artifact.origin with
      | Found _ -> ()
      | Made (job, _, declared) ->
          grouped.(job) <- (artifact.name, artifact.span, declared) :: grouped.(job))
    value.artifacts;
  read_list
    (fun (i, (call : call)) ->
      let outputs = List.rev grouped.(i) in
      let declared = match outputs with (_, _, declared) :: _ -> declared | [] -> None in
      Ok
        {
          span = call.span;
          names = List.map (fun (n, _, _) -> n) outputs;
          name_spans = List.map (fun (_, s, _) -> s) outputs;
          call;
          declared;
        })
    (List.mapi (fun i call -> (i, call)) value.jobs)

let validate_settings (operations : (string, operation_decl) Hashtbl.t) settings =
  let+ _ =
    read_list
      (fun (s : Setting.t located) ->
        match s.value with
        | Setting.Command (Named_operation name, _)
        | Verify (Named_operation name, _)
        | Calculate (Named_operation name, _)
        | Resources (Named_operation name, _) -> (
            match Hashtbl.find_opt operations name with
            | Some { body = Some _; _ } -> error ~span:s.span (Problem.Composite_settings name)
            | _ -> Ok ())
        | _ -> Ok ())
      settings
  in
  ()

let rec expand_scope operations contracts (value : scope) =
  let* bindings = scope_bindings value in
  let* bindings = read_list (expand_binding operations contracts) bindings in
  let* stages =
    read_list
      (fun (s : stage) ->
        let+ scope = expand_scope operations contracts s.scope in
        { s with scope })
      value.stages
  in
  let+ () = validate_settings operations value.settings in
  Scope.of_bindings
    ~operations:(List.filter (fun (o : operation_decl) -> o.body = None) value.operations)
    ~sources:
      (List.filter
         (fun (a : Ast.artifact) -> match a.origin with Found _ -> true | _ -> false)
         value.artifacts)
    ~settings:value.settings ~stages (List.concat bindings)

let expand program =
  let operations = operation_index program and contracts = ref [] in
  let* _ =
    Hashtbl.fold (fun _ operation acc -> operation :: acc) operations []
    |> List.sort (fun (a : operation_decl) b -> String.compare a.name b.name)
    |> read_list validate
  in
  let* () = check_cycles operations in
  let* global = expand_scope operations contracts program.syntax.global in
  let+ libraries = read_list (expand_scope operations contracts) program.libraries in
  {
    program with
    syntax = { program.syntax with global };
    libraries;
    contracts = List.rev !contracts;
  }
