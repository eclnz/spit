open Ast
open Data

type exports = {
  operations : (string, string) Hashtbl.t;
  artifacts : (string, string) Hashtbl.t;
  gates : (string, string) Hashtbl.t;
  types : (string, string) Hashtbl.t;
}

type unit_ = {
  path : string;
  source : string;
  prefix : string;
  display : string;
  pipeline : pipeline;
  imports : use_decl list;
  exports : exports;
}

type task = Read of string * string * span option | Finish of string

let absolute base path =
  let path = if Filename.is_relative path then Filename.concat base path else path in
  let components =
    List.fold_left
      (fun acc -> function
        | "" | "." -> acc
        | ".." -> ( match acc with [] -> [] | _ :: rest -> rest)
        | part -> part :: acc)
      [] (String.split_on_char '/' path)
  in
  "/" ^ String.concat "/" (List.rev components)

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let empty_exports () =
  {
    operations = Hashtbl.create 16;
    artifacts = Hashtbl.create 16;
    gates = Hashtbl.create 8;
    types = Hashtbl.create 8;
  }

let gate_name = function Check t | Screen t -> t.name | Review name -> name

let own_exports prefix pipeline =
  let exports = empty_exports () in
  let pending = Stack.create () in
  Stack.push pipeline.global pending;
  while not (Stack.is_empty pending) do
    let scope = Stack.pop pending in
    List.iter
      (fun (o : operation_decl) -> Hashtbl.replace exports.operations o.name (prefix ^ o.name))
      scope.operations;
    List.iter
      (fun (a : Ast.artifact) ->
        match a.origin with
        | Found _ -> Hashtbl.replace exports.artifacts a.name (prefix ^ a.name)
        | Made _ -> ())
      scope.artifacts;
    List.iter (fun (s : stage) -> Stack.push s.scope pending) scope.stages
  done;
  List.iter
    (fun gate ->
      let name = gate_name gate in
      Hashtbl.replace exports.gates name (prefix ^ name))
    pipeline.gates;
  List.iter
    (fun (d : type_def) -> Hashtbl.replace exports.types d.name (prefix ^ d.name))
    pipeline.type_defs;
  exports

let add_import environment dependency (declaration : use_decl) =
  let tables e = [ e.operations; e.artifacts; e.gates; e.types ] in
  let selected name =
    match declaration.names with None -> true | Some names -> List.mem name names
  in
  let* () =
    match declaration.names with
    | None -> Ok ()
    | Some names ->
        let+ _ =
          read_list
            (fun name ->
              if List.exists (fun table -> Hashtbl.mem table name) (tables dependency.exports) then
                Ok ()
              else error ~span:declaration.span (Problem.Import_missing name))
            names
        in
        ()
  in
  let pairs = List.combine (tables environment) (tables dependency.exports) in
  let* _ =
    read_list
      (fun (target, source) ->
        let entries =
          Hashtbl.fold
            (fun name actual acc -> if selected name then (name, actual) :: acc else acc)
            source []
          |> List.sort compare
        in
        let+ _ =
          read_list
            (fun (name, actual) ->
              let visible =
                match declaration.alias with None -> name | Some alias -> alias ^ "." ^ name
              in
              if Hashtbl.mem target visible then
                error ~span:declaration.span (Problem.Import_collision visible)
              else (
                Hashtbl.add target visible actual;
                Ok ()))
            entries
        in
        ())
      pairs
  in
  Ok ()

let require unresolved table span unknown reference =
  let name = ref_name reference in
  match Hashtbl.find_opt table name with
  | Some actual -> { namespace = None; name = actual }
  | None ->
      unresolved := { span = Some span; file = None; problem = unknown name } :: !unresolved;
      { namespace = None; name = "\000" ^ name }

let renamers environment prefix =
  {
    Rename.operation =
      (fun name ->
        Option.value (Hashtbl.find_opt environment.operations name) ~default:(prefix ^ name));
    artifact =
      (fun name ->
        Option.value (Hashtbl.find_opt environment.artifacts name) ~default:(prefix ^ name));
    gate =
      (fun name -> Option.value (Hashtbl.find_opt environment.gates name) ~default:(prefix ^ name));
    typ = (fun name -> Option.value (Hashtbl.find_opt environment.types name) ~default:name);
  }

let resolve_call unresolved environment (call : call) =
  let callee =
    require unresolved environment.operations call.callee_span
      (fun name -> Problem.Unknown_operation name)
      call.callee
  in
  let args =
    List.map
      (fun (arg : arg) ->
        {
          arg with
          artifact =
            require unresolved environment.artifacts arg.artifact_span
              (fun name -> Problem.Unknown_input name)
              arg.artifact;
        })
      call.args
  in
  Ok { call with callee; args }

let resolve_operation unresolved environment names (operation : operation_decl) =
  let* body =
    match operation.body with
    | None -> Ok None
    | Some body ->
        let+ body =
          read_list
            (fun (binding : binding) ->
              let+ callee =
                Ok
                  (require unresolved environment.operations binding.call.callee_span
                     (fun name -> Problem.Unknown_operation name)
                     binding.call.callee)
              in
              {
                binding with
                call = { binding.call with callee };
                declared = Option.map (Rename.declared names) binding.declared;
              })
            body
        in
        Some body
  in
  let rewritten = Rename.operation names { operation with body = None } in
  Ok { rewritten with body }

let rec rewrite_scope unresolved environment names (node : scope) =
  let* jobs = read_list (resolve_call unresolved environment) node.jobs in
  let* operations = read_list (resolve_operation unresolved environment names) node.operations in
  let+ stages =
    read_list
      (fun (s : stage) ->
        let+ scope = rewrite_scope unresolved environment names s.scope in
        { s with scope })
      node.stages
  in
  {
    operations;
    artifacts = List.map (Rename.artifact names) node.artifacts;
    jobs;
    settings =
      List.map
        (fun (s : Setting.t located) -> { s with value = Rename.setting names s.value })
        node.settings;
    stages;
  }

let rewrite unresolved environment prefix pipeline =
  let names = renamers environment prefix in
  let+ global = rewrite_scope unresolved environment names pipeline.global in
  {
    pipeline with
    imports = [];
    global;
    gates = List.map (Rename.gate names) pipeline.gates;
    type_defs =
      List.map (fun (d : type_def) -> { d with name = names.typ d.name }) pipeline.type_defs;
  }

type loading = {
  root : string;
  units : (string, unit_) Hashtbl.t;
  visiting : (string, unit) Hashtbl.t;
  order : string list ref;
  counter : int ref;
  stack : task Stack.t;
  syntax : Data.error list ref;
  unresolved : Data.error list ref;
}

let read_unit state path display location =
  if Hashtbl.mem state.visiting path then error ?span:location (Problem.Import_cycle path)
  else if Hashtbl.mem state.units path then Ok ()
  else
    let* source =
      try Ok (read_file path)
      with Sys_error message -> error ?span:location (Problem.Read_failed { path; message })
    in
    let pipeline, diagnostics = Parser.parse source in
    List.iter
      (fun (d : diagnostic) ->
        state.syntax :=
          { span = Some d.span; file = None; problem = Problem.Syntax (path, d.problem) }
          :: !(state.syntax))
      diagnostics;
    let prefix =
      if path = state.root then ""
      else
        let id = !(state.counter) in
        incr state.counter;
        "$module" ^ string_of_int id ^ "."
    in
    let exports = own_exports prefix pipeline in
    let unit_ = { path; source; prefix; display; pipeline; imports = pipeline.imports; exports } in
    Hashtbl.add state.units path unit_;
    Hashtbl.add state.visiting path ();
    Stack.push (Finish path) state.stack;
    List.iter
      (fun (declaration : use_decl) ->
        let dependency = absolute (Filename.dirname path) declaration.from in
        let name =
          Option.value declaration.alias
            ~default:(Filename.remove_extension (Filename.basename dependency))
        in
        let display = if display = "" then name else display ^ "." ^ name in
        Stack.push (Read (dependency, display, Some declaration.span)) state.stack)
      (List.rev pipeline.imports);
    Ok ()

let finish_unit state path =
  let unit_ = Hashtbl.find state.units path in
  let environment = own_exports unit_.prefix unit_.pipeline in
  let pending = Stack.create () in
  Stack.push unit_.pipeline.global pending;
  while not (Stack.is_empty pending) do
    let scope = Stack.pop pending in
    List.iter
      (fun (a : Ast.artifact) ->
        Hashtbl.replace environment.artifacts a.name (unit_.prefix ^ a.name))
      scope.artifacts;
    List.iter (fun (s : stage) -> Stack.push s.scope pending) scope.stages
  done;
  let* _ =
    read_list
      (fun (declaration : use_decl) ->
        let dependency =
          Hashtbl.find state.units (absolute (Filename.dirname path) declaration.from)
        in
        add_import environment dependency declaration)
      unit_.imports
  in
  let* pipeline = rewrite state.unresolved environment unit_.prefix unit_.pipeline in
  Hashtbl.replace state.units path { unit_ with pipeline };
  Hashtbl.remove state.visiting path;
  state.order := path :: !(state.order);
  Ok ()

let load_units syntax unresolved path =
  let root = absolute (Sys.getcwd ()) path in
  let units = Hashtbl.create 16
  and visiting = Hashtbl.create 16
  and order = ref []
  and counter = ref 0
  and stack = Stack.create () in
  let state = { root; units; visiting; order; counter; stack; syntax; unresolved } in
  Stack.push (Read (root, "", None)) stack;
  let rec execute () =
    if Stack.is_empty stack then Ok ()
    else
      let* () =
        match Stack.pop stack with
        | Read (path, display, location) -> read_unit state path display location
        | Finish path -> finish_unit state path
      in
      execute ()
  in
  let* () = execute () in
  let root_unit = Hashtbl.find units root in
  let public = empty_exports () in
  let* _ =
    read_list
      (fun (declaration : use_decl) ->
        add_import public
          (Hashtbl.find units (absolute (Filename.dirname root) declaration.from))
          declaration)
      root_unit.imports
  in
  Ok (root_unit.pipeline, public, units, List.rev !order)

let operation_target_name = function
  | Setting.Command (Named_operation name, _)
  | Verify (Named_operation name, _)
  | Calculate (Named_operation name, _)
  | Resources (Named_operation name, _)
  | Uses (Named_operation name, _)
  | Anchored (Named_operation name)
  | Gate (_, Input_of (name, _), _)
  | Gate (_, Output_of (name, _), _) ->
      Some name
  | _ -> None

let visit_scope read pipeline =
  let pending = Stack.create () in
  Stack.push pipeline.global pending;
  while not (Stack.is_empty pending) do
    let scope = Stack.pop pending in
    read scope;
    List.iter (fun (s : stage) -> Stack.push s.scope pending) scope.stages
  done

let required_setting selected_operations selected_sources (s : Setting.t located) =
  match operation_target_name s.value with
  | Some name -> Hashtbl.mem selected_operations name
  | None -> (
      match s.value with
      | Setting.Path (Named_artifact name, _)
      | Ext (Named_artifact name, _)
      | Gate (_, Named_artifact name, _) ->
          Hashtbl.mem selected_sources name
      | Path (Every_artifact, _) | Ext (Every_artifact, _) | Entities _ -> true
      | Uses (Every_operation, _)
      | Resources (Every_operation, _)
      | Verify (Every_operation, _)
      | Calculate (Every_operation, _)
      | Anchored Every_operation ->
          true
      | _ -> false)

let rec select_scope selected_operations selected_sources selected_gates (scope : scope) =
  let settings =
    List.filter (required_setting selected_operations selected_sources) scope.settings
  in
  List.iter
    (fun (s : Setting.t located) ->
      match s.value with
      | Setting.Gate (_, _, uses) ->
          List.iter (fun use -> Hashtbl.replace selected_gates (ref_name use.gate) ()) uses
      | _ -> ())
    settings;
  {
    operations =
      List.filter
        (fun (o : operation_decl) -> Hashtbl.mem selected_operations o.name)
        scope.operations;
    artifacts =
      List.filter (fun (a : Ast.artifact) -> Hashtbl.mem selected_sources a.name) scope.artifacts;
    jobs = [];
    settings;
    stages =
      List.map
        (fun (s : stage) ->
          {
            s with
            scope = select_scope selected_operations selected_sources selected_gates s.scope;
          })
        scope.stages;
  }

let operation_closure (operations : (string, operation_decl) Hashtbl.t) root public =
  let selected = Hashtbl.create 32 and pending = Queue.create () in
  visit_scope
    (fun scope -> List.iter (fun (o : operation_decl) -> Queue.add o.name pending) scope.operations)
    root;
  Hashtbl.iter (fun _ name -> Queue.add name pending) public.operations;
  while not (Queue.is_empty pending) do
    let name = Queue.take pending in
    if not (Hashtbl.mem selected name) then (
      Hashtbl.add selected name ();
      match Hashtbl.find_opt operations name with
      | Some { body = Some bindings; _ } ->
          List.iter (fun binding -> Queue.add (ref_name binding.call.callee) pending) bindings
      | _ -> ())
  done;
  selected

let imported_scopes root public units order =
  let operations = Hashtbl.create 32 in
  List.iter
    (fun path ->
      visit_scope
        (fun scope ->
          List.iter
            (fun (o : operation_decl) -> Hashtbl.replace operations o.name o)
            scope.operations)
        (Hashtbl.find units path).pipeline)
    order;
  let selected_operations = operation_closure operations root public in
  let selected_sources = Hashtbl.create 16 and selected_gates = Hashtbl.create 16 in
  Hashtbl.iter (fun _ name -> Hashtbl.replace selected_sources name ()) public.artifacts;
  Hashtbl.iter (fun _ name -> Hashtbl.replace selected_gates name ()) public.gates;
  List.iter (fun gate -> Hashtbl.replace selected_gates (gate_name gate) ()) root.gates;
  (selected_gates, select_scope selected_operations selected_sources selected_gates)

let display_name preferred used imported name =
  match Hashtbl.find_opt preferred name with
  | Some name -> name
  | None -> (
      match List.find_opt (fun unit_ -> String.starts_with ~prefix:unit_.prefix name) imported with
      | None -> name
      | Some unit_ ->
          let local =
            String.sub name (String.length unit_.prefix)
              (String.length name - String.length unit_.prefix)
          in
          let candidate = unit_.display ^ "." ^ local in
          let candidate =
            match Hashtbl.find_opt used candidate with
            | Some actual when actual <> name -> name
            | _ -> candidate
          in
          Hashtbl.add preferred name candidate;
          Hashtbl.replace used candidate name;
          candidate)

let display_names public imported root =
  let preferred = Hashtbl.create 32 and used = Hashtbl.create 32 in
  let add visible actual =
    if not (Hashtbl.mem preferred actual) then (
      Hashtbl.add preferred actual visible;
      Hashtbl.replace used visible actual)
  in
  List.iter
    (fun table ->
      Hashtbl.fold (fun visible actual acc -> (visible, actual) :: acc) table []
      |> List.sort compare
      |> List.iter (fun (visible, actual) -> add visible actual))
    [ public.operations; public.artifacts; public.gates; public.types ];
  visit_scope
    (fun scope ->
      List.iter (fun (o : operation_decl) -> Hashtbl.replace used o.name o.name) scope.operations;
      List.iter (fun (a : Ast.artifact) -> Hashtbl.replace used a.name a.name) scope.artifacts)
    root;
  display_name preferred used imported

let program syntax unresolved path =
  let* root, public, units, order = load_units syntax unresolved path in
  let selected_gates, select_scope = imported_scopes root public units order in
  let root_path = absolute (Sys.getcwd ()) path in
  let imported = List.filter (( <> ) root_path) order |> List.map (Hashtbl.find units) in
  let scopes = List.map (fun unit_ -> select_scope unit_.pipeline.global) imported in
  let display = display_names public imported root in
  let names = { Rename.operation = display; artifact = display; gate = display; typ = display } in
  let gates =
    root.gates
    @ List.concat_map
        (fun unit_ ->
          List.filter (fun gate -> Hashtbl.mem selected_gates (gate_name gate)) unit_.pipeline.gates)
        imported
  in
  let type_defs =
    root.type_defs @ List.concat_map (fun unit_ -> unit_.pipeline.type_defs) imported
  in
  let files =
    List.map
      (fun path -> (path, (Hashtbl.find units path).source))
      (root_path :: List.filter (( <> ) root_path) order)
  in
  Ok
    {
      syntax =
        {
          root with
          global = Rename.scope names root.global;
          imports = [];
          gates = List.map (Rename.gate names) gates;
          type_defs = List.map (fun (d : type_def) -> { d with name = display d.name }) type_defs;
        };
      libraries = List.map (Rename.scope names) scopes;
      files;
      contracts = [];
    }

let load path =
  let syntax = ref [] and unresolved = ref [] in
  let result = program syntax unresolved path in
  match (List.rev !syntax, result) with
  | [], Ok program -> Ok (program, List.rev !unresolved)
  | [], Error e -> Error (List.rev !unresolved @ [ e ])
  | errors, _ -> Error errors
