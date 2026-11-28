open Ast
open Data

type raw_step = {
  call : call;
  names : (string * declared option) array;
  stage : string list;
  settings : Setting.t located list;
}

type declarations = {
  operations : operation_decl list;
  sources : (Ast.artifact * Setting.t located list) list;
  steps : raw_step list;
  settings : Setting.t located list;
  owners : (string, Setting.t located list) Hashtbl.t;
}

let collect program =
  let pipeline = program.syntax in
  let operations = ref [] and sources = ref [] and steps = ref [] and all_settings = ref [] in
  let owners = Hashtbl.create 32 in
  let pending = Stack.create () in
  List.iter (fun scope -> Stack.push (scope, [], []) pending) (List.rev program.libraries);
  Stack.push (pipeline.global, [], []) pending;
  while not (Stack.is_empty pending) do
    let scope, stage, inherited = Stack.pop pending in
    let settings = inherited @ scope.settings in
    operations := List.rev_append scope.operations !operations;
    List.iter
      (fun (operation : operation_decl) -> Hashtbl.replace owners operation.name settings)
      scope.operations;
    all_settings := List.rev_append scope.settings !all_settings;
    let calls = Array.of_list scope.jobs in
    let outputs = Array.make (Array.length calls) [] in
    List.iter
      (fun (a : Ast.artifact) ->
        match a.origin with
        | Found _ -> sources := (a, settings) :: !sources
        | Made (index, _, declared) -> outputs.(index) <- (a.name, declared) :: outputs.(index))
      scope.artifacts;
    Array.iteri
      (fun i call ->
        steps := { call; names = Array.of_list (List.rev outputs.(i)); stage; settings } :: !steps)
      calls;
    List.iter
      (fun (s : stage) -> Stack.push (s.scope, stage @ [ s.name ], settings) pending)
      (List.rev scope.stages)
  done;
  {
    operations = List.rev !operations;
    sources = List.rev !sources;
    steps = List.rev !steps;
    settings = List.rev !all_settings;
    owners;
  }

let supported problems pipeline declarations =
  if pipeline.imports <> [] then
    ignore (keep problems (error (Problem.Not_implemented ("use", "imports are"))))
  else if List.exists (function Ast.Check _ -> false | _ -> true) pipeline.gates then
    ignore (keep problems (error (Problem.Not_implemented ("pipeline", "gates and tools are"))))
  else
    let _ =
      each problems
        (fun (s : Setting.t located) ->
          match s.value with
          | Setting.Path ((Every_artifact | Named_artifact _), _)
          | Ext (Every_artifact, Replace _)
          | Entities (Every_dimension, Format _)
          | Entities (Named_dimension _, Rename _)
          | Command (Named_operation _, _)
          | Verify (Named_operation _, _)
          | Calculate (Named_operation _, _) ->
              Ok ()
          | Uses (target, uses) -> (
              if not (List.for_all (fun used -> List.mem used.tool pipeline.tools) uses) then
                error Problem.Undeclared_tool
              else
                match target with
                | Named_operation name
                  when not
                         (List.exists
                            (fun (operation : operation_decl) -> operation.name = name)
                            declarations.operations) ->
                    error (Problem.Uses_unknown_operation name)
                | _ -> Ok ())
          | Gate (Gate_kind.Check, _, _) | Resources _ | Anchored _ -> Ok ()
          | _ -> error (Problem.Not_implemented ("setting", "this setting is")))
        declarations.settings
    in
    ()

let unique table subject name value =
  if Hashtbl.mem table name then error (Problem.Duplicate_name { subject; name })
  else (
    Hashtbl.add table name value;
    Ok ())

let implementations problems declarations operations names =
  let commands = Hashtbl.create 32
  and verifies = Hashtbl.create 16
  and calculations = Hashtbl.create 16 in
  let _ =
    each problems
      (fun (s : Setting.t located) ->
        match s.value with
        | Setting.Command (Named_operation name, lines) | Verify (Named_operation name, lines) ->
            if not (Hashtbl.mem operations name) then error (Problem.Setting_unknown_operation name)
            else
              let table = match s.value with Setting.Command _ -> commands | _ -> verifies in
              if Hashtbl.mem calculations name && table == commands then
                error (Problem.Command_and_calculate name)
              else unique table name name lines
        | Calculate (Named_operation name, expression) -> (
            match Hashtbl.find_opt operations name with
            | None -> error (Problem.Setting_unknown_operation name)
            | Some (operation : operation_decl) ->
                if Hashtbl.mem commands name then error (Problem.Calculate_and_command name)
                else if
                  match operation.outputs with [ { shape = File _; _ } ] -> false | _ -> true
                then error (Problem.Calculate_output name)
                else
                  let* expression = Calculation.parse operation expression in
                  unique calculations name name expression)
        | Resources (Named_operation name, _)
        | Uses (Named_operation name, _)
        | Anchored (Named_operation name) ->
            if Hashtbl.mem operations name then Ok ()
            else error (Problem.Setting_unknown_operation name)
        | Path (Named_artifact name, _) | Ext (Named_artifact name, _) ->
            if Hashtbl.mem names name then Ok () else error (Problem.Setting_unknown_product name)
        | _ -> Ok ())
      declarations.settings
  in
  (commands, verifies, calculations)

let is_folder = function Folder | Folder_from _ -> true | File _ | View -> false

type context = {
  strict : bool;
  definitions : (string, int) Hashtbl.t;
  operations : (string, operation_decl) Hashtbl.t;
  names : (string, Product_id.t) Hashtbl.t;
  symbols : symbols;
  products : product option array;
  dimension_order : Symbol.t list ref;
  commands : (string, script) Hashtbl.t;
  verifies : (string, script) Hashtbl.t;
  calculations : (string, Json.t) Hashtbl.t;
  configurations : (string, configuration) Hashtbl.t;
}

let validate_param strict definitions subject (p : param) =
  let* () =
    if List.exists (fun (Min minimum) -> minimum < 1 || not p.many) p.constraints then
      error (Problem.Minimum_bound p.name)
    else Ok ()
  in
  Types.check_declared ~strict definitions subject p.typ

let validate_operation strict definitions operations (o : operation_decl) =
  let distinct names = List.length (List.sort_uniq String.compare names) = List.length names in
  let* () =
    if
      distinct (List.map (fun (p : param) -> p.name) o.params)
      && distinct (List.map port_name o.outputs)
    then Ok ()
    else error (Problem.Duplicate_port o.name)
  in
  let* () =
    if o.body = None then Ok ()
    else error (Problem.Not_implemented (o.name, "composite expansion is"))
  in
  let* _ = read_list (validate_param strict definitions o.name) o.params in
  let* _ =
    read_list
      (fun (o : output) -> Types.check_declared ~strict definitions "output" o.typ)
      o.outputs
  in
  unique operations "operation" o.name o

let register_products problems names declarations =
  let next = ref 0 in
  let add name =
    let id = Product_id.Product_id !next in
    let+ () = unique names "product" name id in
    incr next;
    id
  in
  let sources = each problems (fun ((a : Ast.artifact), _) -> add a.name) declarations.sources in
  List.iter
    (fun (step : raw_step) ->
      ignore (each problems (fun (name, _) -> add name) (Array.to_list step.names)))
    declarations.steps;
  (sources, !next)

let dimension ctx name =
  let id = intern ctx.symbols name in
  if not (List.mem id !(ctx.dimension_order)) then
    ctx.dimension_order := !(ctx.dimension_order) @ [ id ];
  id

let ordered ctx dims =
  Array.of_list (List.filter (fun dim -> List.mem dim dims) !(ctx.dimension_order))

let source_dimensions ctx settings (a : Ast.artifact) (found : found) =
  let* () =
    if Option.fold ~none:false ~some:Types.has_variable found.typ then
      error (Problem.Source_type_variable a.name)
    else Ok ()
  in
  let declared = match found.placement with Dims dims -> dims | Here -> [] in
  let* dims =
    match
      ( declared,
        Option.map (fun (p : pattern) -> references p.segments) (rule_pattern a.name settings) )
    with
    | [], Some (reference :: _) -> (
        match Hashtbl.find_opt ctx.names reference with
        | Some id -> (
            match ctx.products.(Product_id.index id) with
            | Some product -> Ok (List.map (symbol_text ctx.symbols) (Array.to_list product.dims))
            | None -> error (Problem.Reference_not_source { product = a.name; reference }))
        | None -> error (Problem.Reference_unknown { product = a.name; reference }))
    | dims, _ -> Ok dims
  in
  let* () = Types.check_declared ~strict:ctx.strict ctx.definitions a.name found.typ in
  let* () =
    if ctx.strict && found.typ = None then error (Problem.Strict_source_type a.name) else Ok ()
  in
  let dims = List.map (dimension ctx) dims in
  if List.length (List.sort_uniq compare dims) <> List.length dims then
    error (Problem.Duplicate_source_dimension a.name)
  else Ok (ordered ctx dims)

let add_source ctx ((a : Ast.artifact), settings) =
  match a.origin with
  | Made _ -> invalid_arg "source collection contains only found artifacts"
  | Found found ->
      let+ dims = source_dimensions ctx settings a found in
      let id = Hashtbl.find ctx.names a.name in
      ctx.products.(Product_id.index id) <-
        Some
          {
            id;
            here = found.placement = Here;
            name = a.name;
            path_name = Span.text a.span;
            typ = found.typ;
            dims;
            shape = found.shape;
            stage = [];
            settings;
          }

let dependencies problems names raw_steps =
  let producer = Hashtbl.create (Array.length raw_steps) in
  Array.iteri
    (fun i (step : raw_step) ->
      Array.iter (fun (name, _) -> Hashtbl.add producer name i) step.names)
    raw_steps;
  Array.map
    (fun (step : raw_step) ->
      let inputs =
        read_list
          (fun (arg : arg) ->
            let name = ref_name arg.artifact in
            if Hashtbl.mem names name then Ok (Hashtbl.find_opt producer name)
            else error ~span:arg.artifact_span (Problem.Unknown_input name))
          step.call.args
      in
      Option.map
        (fun inputs -> List.sort_uniq Int.compare (List.filter_map Fun.id inputs))
        (keep problems inputs))
    raw_steps

let dependency_order dependencies =
  let usable dep = dependencies.(dep) <> None in
  let dependencies = Array.map (Option.map (List.filter usable)) dependencies in
  let remaining = Array.map (fun deps -> List.length (Option.value deps ~default:[])) dependencies
  and consumers = Array.make (Array.length dependencies) [] in
  Array.iteri
    (fun i deps ->
      List.iter (fun dep -> consumers.(dep) <- i :: consumers.(dep)) (Option.value deps ~default:[]))
    dependencies;
  let ready = Queue.create () and order = ref [] in
  Array.iteri
    (fun i count -> if count = 0 && dependencies.(i) <> None then Queue.add i ready)
    remaining;
  while not (Queue.is_empty ready) do
    let i = Queue.take ready in
    order := i :: !order;
    List.iter
      (fun next ->
        remaining.(next) <- remaining.(next) - 1;
        if remaining.(next) = 0 then Queue.add next ready)
      (List.rev consumers.(i))
  done;
  let usable = Array.fold_left (fun n deps -> if deps = None then n else n + 1) 0 dependencies in
  (List.rev !order, List.length !order <> usable)

let get ctx id =
  match ctx.products.(Product_id.index id) with
  | Some product -> product
  | None -> invalid_arg "dependency order supplies each input product"

let driver name (params : param array) selections =
  let indices = List.init (Array.length params) Fun.id in
  let many = List.filter (fun i -> params.(i).many) indices in
  let candidates = List.filter (fun i -> Array.length selections.(i).each = 0) indices in
  let chosen =
    match (many, candidates) with
    | i :: _, _ -> Some i
    | [], first :: rest ->
        Some
          (List.fold_left
             (fun best i ->
               if Array.length selections.(i).matching > Array.length selections.(best).matching
               then i
               else best)
             first rest)
    | [], [] -> None
  in
  let* () =
    if chosen = None && params <> [||] then error (Problem.Broadcast_cannot_drive name) else Ok ()
  in
  let varied =
    match many with first :: _ -> Array.to_list selections.(first).varying | [] -> []
  in
  let+ _ =
    read_list
      (fun i ->
        if Array.to_list selections.(i).varying <> varied then error (Problem.Parallel_vary name)
        else if selections.(i).effective <> selections.(Option.get chosen).effective then
          error (Problem.Parallel_dimensions name)
        else Ok ())
      many
  in
  (chosen, Array.of_list varied)

let declared_type ctx name (declared : declared option) typ dims =
  match declared with
  | None -> Ok ()
  | Some d -> (
      let* () = Types.check_declared ~strict:ctx.strict ctx.definitions name d.typ in
      let* () =
        if Option.fold ~none:false ~some:Types.has_variable d.typ then
          error (Problem.Declaration_type_variable name)
        else Ok ()
      in
      let* () = Types.unify name (Hashtbl.create 0) d.typ typ in
      match d.dims with
      | None -> Ok ()
      | Some declared ->
          if List.map (intern ctx.symbols) declared = Array.to_list dims then Ok ()
          else error (Problem.Declared_dimensions name))

let check_output ctx (raw : raw_step) substitutions dims (i, (output : output)) =
  let name, declared = raw.names.(i) in
  let typ = Types.infer_output substitutions output in
  let* () = declared_type ctx name declared typ dims in
  let typ = match declared with Some { typ = Some _ as typ; _ } -> typ | _ -> typ in
  let operation = Hashtbl.find ctx.operations (ref_name raw.call.callee) in
  let* () =
    if ctx.strict && (typ = None || List.exists (fun (p : param) -> p.typ = None) operation.params)
    then error (Problem.Strict_port_types name)
    else Ok ()
  in
  let id = Hashtbl.find ctx.names name in
  ctx.products.(Product_id.index id) <-
    Some
      {
        id;
        here = false;
        name;
        path_name = String.concat "." (String.split_on_char ':' name |> List.filter (( <> ) ""));
        typ;
        dims;
        shape = output.shape;
        stage = raw.stage;
        settings = raw.settings;
      };
  Ok id

let view (operation : operation_decl) =
  List.exists (fun (o : output) -> o.shape = View) operation.outputs

let implementation ctx (operation : operation_decl) inputs dims =
  let calculation = Hashtbl.find_opt ctx.calculations operation.name in
  let* commands =
    match Hashtbl.find_opt ctx.commands operation.name with
    | Some _ when view operation -> error (Problem.View_command operation.name)
    | Some script -> Ok script.commands
    | None when calculation <> None || view operation -> Ok []
    | None -> error (Problem.No_command operation.name)
  in
  let verify =
    Option.fold ~none:[]
      ~some:(fun (script : script) -> script.commands)
      (Hashtbl.find_opt ctx.verifies operation.name)
  in
  let input_dims = Array.map (fun id -> (get ctx id).dims) inputs in
  let* () = Command_check.validate ~verify:false operation input_dims dims ctx.symbols commands in
  let+ () = Command_check.validate ~verify:true operation input_dims dims ctx.symbols verify in
  (commands, verify, calculation)

let check_step ctx (raw : raw_step) =
  let name = ref_name raw.call.callee in
  let* operation =
    match Hashtbl.find_opt ctx.operations name with
    | Some op -> Ok op
    | None -> error ~span:raw.call.callee_span (Problem.Unknown_operation name)
  in
  let params = Array.of_list operation.params and args = Array.of_list raw.call.args in
  let* () =
    if Array.length params <> Array.length args then error (Problem.Argument_count name)
    else if List.length operation.outputs <> Array.length raw.names then
      error (Problem.Output_count name)
    else Ok ()
  in
  let inputs = Array.map (fun (a : arg) -> Hashtbl.find ctx.names (ref_name a.artifact)) args in
  let indices = List.init (Array.length params) Fun.id in
  let* selections =
    read_list
      (fun i -> Selection.describe ctx.symbols (get ctx inputs.(i)).dims params.(i) args.(i))
      indices
  in
  let selections = Array.of_list selections in
  let* driver, varying = driver name params selections in
  let names = Array.map (fun (a : arg) -> ref_name a.artifact) args in
  let* dims = Selection.dimensions ctx.symbols names !(ctx.dimension_order) selections driver in
  let substitutions = Hashtbl.create 8 in
  let* _ =
    read_list
      (fun i ->
        Types.unify params.(i).name substitutions params.(i).typ (get ctx inputs.(i)).typ
        |> located args.(i).artifact_span)
      indices
  in
  let* () =
    if
      view operation
      && (List.length operation.outputs <> 1 || Array.exists (fun (p : param) -> p.many) params)
    then error (Problem.View_shape name)
    else Ok ()
  in
  let* _ =
    read_list
      (fun (o : output) ->
        match o.shape with
        | Folder_from port -> (
            match List.find_index (fun (p : param) -> p.name = port) operation.params with
            | Some i when (not params.(i).many) && is_folder (get ctx inputs.(i)).shape -> Ok ()
            | _ -> error (Problem.Seed_port { operation = name; port }))
        | _ -> Ok ())
      operation.outputs
  in
  let* outputs =
    read_list
      (check_output ctx raw substitutions dims)
      (List.mapi (fun i o -> (i, o)) operation.outputs)
  in
  let+ commands, verify, calculation = implementation ctx operation inputs dims in
  let minimum =
    Array.fold_left
      (fun n (p : param) -> List.fold_left (fun n (Min m) -> max n m) n p.constraints)
      1 params
  in
  {
    settings = raw.settings;
    operation;
    calculation;
    configuration = Hashtbl.find ctx.configurations name;
    selections;
    inputs;
    outputs = Array.of_list outputs;
    args;
    varying;
    driver;
    minimum;
    stage = raw.stage;
    commands;
    verify;
  }

let contract_output ctx (contract : composite_contract) substitutions ((output : output), name) =
  let actual = get ctx (Hashtbl.find ctx.names name) in
  let* () =
    Types.unify name (Hashtbl.create 0) (Types.infer_output substitutions output) actual.typ
    |> located contract.span
  in
  match contract.declared with
  | None -> Ok ()
  | Some declaration -> (
      let* () =
        Types.unify name (Hashtbl.create 0) declaration.typ actual.typ |> located contract.span
      in
      match declaration.dims with
      | None -> Ok ()
      | Some dimensions ->
          if List.map (intern ctx.symbols) dimensions = Array.to_list actual.dims then Ok ()
          else error ~span:contract.span (Problem.Composite_dimensions name))

let check_contract ctx (contract : composite_contract) =
  let substitutions = Hashtbl.create 8 in
  let* _ =
    read_list
      (fun ((param : param), (arg : arg)) ->
        Types.unify param.name substitutions param.typ
          (get ctx (Hashtbl.find ctx.names (ref_name arg.artifact))).typ
        |> located arg.artifact_span)
      (List.combine contract.operation.params contract.args)
  in
  let+ _ =
    read_list
      (contract_output ctx contract substitutions)
      (List.combine contract.operation.outputs contract.outputs)
  in
  ()

let context problems program (declarations : declarations) =
  let pipeline = program.syntax in
  let definitions = Hashtbl.create 16
  and operations = Hashtbl.create 32
  and names = Hashtbl.create 64 in
  let strict = pipeline.types = Some Strict in
  let _ =
    each problems
      (fun (d : type_def) -> unique definitions "type" d.name (List.length d.params))
      pipeline.type_defs
  in
  let _ =
    each problems (validate_operation strict definitions operations) declarations.operations
  in
  let sources, count = register_products problems names declarations in
  let commands, verifies, calculations = implementations problems declarations operations names in
  let+ configurations =
    Execution.configurations declarations.owners declarations.settings declarations.operations
  in
  ( {
      strict;
      definitions;
      operations;
      names;
      symbols = symbols ();
      products = Array.make count None;
      dimension_order = ref [];
      commands;
      verifies;
      calculations;
      configurations;
    },
    sources )

let ready ctx (args : arg list) =
  List.for_all
    (fun (arg : arg) ->
      match Hashtbl.find_opt ctx.names (ref_name arg.artifact) with
      | Some id -> ctx.products.(Product_id.index id) <> None
      | None -> false)
    args

let check program =
  let problems = ref [] in
  let declarations = collect program in
  supported problems program.syntax declarations;
  ignore (keep problems (Check_definition.validate program.syntax.gates declarations.settings));
  match keep problems (context problems program declarations) with
  | None -> Error (List.rev !problems)
  | Some (ctx, sources) -> (
      List.iter (fun name -> ignore (dimension ctx name)) program.syntax.dimensions;
      let refers ((a : Ast.artifact), settings) =
        match rule_pattern a.name settings with
        | Some pattern -> references pattern.segments <> []
        | None -> false
      in
      let plain, referring =
        List.partition (fun source -> not (refers source)) declarations.sources
      in
      let _ = each problems (add_source ctx) plain in
      let _ = each problems (add_source ctx) referring in
      let raw_steps = Array.of_list declarations.steps in
      let dependencies = dependencies problems ctx.names raw_steps in
      let order, cyclic = dependency_order dependencies in
      if cyclic then ignore (keep problems (error Problem.Cycle));
      let steps =
        List.filter_map
          (fun i ->
            let raw = raw_steps.(i) in
            let callee = ref_name raw.call.callee in
            let declared_but_failed =
              (not (Hashtbl.mem ctx.operations callee))
              && List.exists
                   (fun (operation : operation_decl) -> operation.name = callee)
                   declarations.operations
            in
            if ready ctx raw.call.args && not declared_but_failed then
              keep problems (check_step ctx raw)
            else None)
          order
      in
      let _ =
        each problems (check_contract ctx)
          (List.filter
             (fun (contract : composite_contract) ->
               ready ctx contract.args
               && List.for_all
                    (fun name ->
                      match Hashtbl.find_opt ctx.names name with
                      | Some id -> ctx.products.(Product_id.index id) <> None
                      | None -> false)
                    contract.outputs)
             program.contracts)
      in
      match !problems with
      | _ :: _ -> Error (List.rev !problems)
      | [] ->
          Ok
            {
              symbols = ctx.symbols;
              products =
                Array.map
                  (function Some p -> p | None -> invalid_arg "every product is checked")
                  ctx.products;
              names = ctx.names;
              sources = Array.of_list sources;
              steps = Array.of_list steps;
              pipeline = program;
            })
