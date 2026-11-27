open Ast
open Data
open Json

let typ =
  let rec encode = function
    | Var v -> Object [ ("variable", Text v) ]
    | Named (n, args) -> Object [ ("name", Text (ref_name n)); ("args", list encode args) ]
  in
  encode

let artifact_json resolved paths id =
  let artifact = Data.artifact resolved id in
  let product = Data.product resolved.checked artifact.product in
  Object
    [
      ("product", Text product.name);
      ( "entities",
        Object
          (Array.to_list artifact.entities
          |> List.map (fun (d, v) ->
              (symbol_text resolved.checked.symbols d, Text (symbol_text resolved.checked.symbols v)))
          ) );
      ("type", option typ product.typ);
      ("path", Text paths.(Artifact_id.index id));
      ("under", Text (if artifact.producer = None then "root" else "output_root"));
      ("kind", Text "file");
    ]

let collection_json resolved (step : step) params =
  if Array.length step.varying = 0 then []
  else
    [
      ( "collection",
        Object
          [
            ( "ports",
              List
                (Array.to_list (Array.mapi (fun i (p : param) -> (i, p.many)) params)
                |> List.filter_map (fun (i, many) -> if many then Some (Number i) else None)) );
            ( "varied",
              strings
                (Array.to_list (Array.map (symbol_text resolved.checked.symbols) step.varying)) );
            ("minimum", Number step.minimum);
          ] );
    ]

let command_json (arguments, stdin, stdout) =
  Object
    [
      ("command", list (fun parts -> List parts) arguments);
      ("stdin", option str stdin);
      ("stdout", option str stdout);
    ]

let job_json resolved paths dependents (job : job) commands verify =
  let step = resolved.checked.steps.(job.step) in
  let params = Array.of_list step.operation.params
  and outputs = Array.of_list step.operation.outputs in
  let inputs =
    Array.to_list
      (Array.mapi
         (fun i ids ->
           let value = list (artifact_json resolved paths) (Array.to_list ids) in
           (params.(i).name, value))
         job.inputs)
  in
  let output_fields =
    Array.to_list
      (Array.mapi
         (fun i id -> (port_name outputs.(i), artifact_json resolved paths id))
         job.outputs)
  in
  Object
    ([
       ("id", Number (Job_id.number job.id));
       ("operation", Text step.operation.name);
       ("stage", strings step.stage);
       ("origin", Object [ ("call", Null); ("file", Null); ("line", Number 1) ]);
       ("inputs", Object inputs);
       ("outputs", Object output_fields);
       ("depends_on", list (fun id -> Number (Job_id.number id)) job.depends_on);
       ( "dependents",
         list (fun id -> Number (Job_id.number id)) (List.rev dependents.(Job_id.number job.id - 1))
       );
       ("commands", list command_json commands);
       ("verify", list (fun (args, _, _) -> list (fun p -> List p) args) verify);
       ("checks", List []);
       ("cores", Number step.configuration.cores);
       ("memory", option (fun n -> Number n) step.configuration.memory);
       ("tools", List []);
       ("anchored", Bool step.configuration.anchored);
     ]
    @ collection_json resolved step params)

let executable executables arguments =
  match arguments with
  | [ Text name ] :: _ when not (String.contains name '/') -> Hashtbl.replace executables name ()
  | _ -> ()

let bind_job resolved paths dependents used executables (job : job) =
  let step = resolved.checked.steps.(job.step) in
  let* commands =
    match step.calculation with
    | None -> Command.bind ~redirect:true resolved paths job step step.commands
    | Some expression ->
        Ok
          [
            ( [ [ Object [ ("calculate", expression) ] ] ],
              None,
              Some paths.(Artifact_id.index job.outputs.(0)) );
          ]
  in
  let* verify = Command.bind ~redirect:false resolved paths job step step.verify in
  let* () =
    if commands = [] then error step.operation.name "operation has no executable command" else Ok ()
  in
  List.iter (fun (arguments, _, _) -> executable executables arguments) (commands @ verify);
  Array.iter (fun ids -> Array.iter (fun id -> Hashtbl.replace used id ()) ids) job.inputs;
  Ok (job_json resolved paths dependents job commands verify)

let dependents resolved =
  let dependents = Array.make (Array.length resolved.jobs) [] in
  Array.iter
    (fun (job : job) ->
      List.iter
        (fun id ->
          let index = Job_id.number id - 1 in
          dependents.(index) <- job.id :: dependents.(index))
        job.depends_on)
    resolved.jobs;
  dependents

let write resolved paths ~root ~output_root ~pipeline_folder =
  let dependents = dependents resolved in
  let used = Hashtbl.create 64 and executables = Hashtbl.create 8 in
  let* jobs =
    read_list (bind_job resolved paths dependents used executables) (Array.to_list resolved.jobs)
  in
  let ids = List.init (Array.length resolved.artifacts) (fun i -> Artifact_id.Artifact_id i) in
  let external_inputs =
    List.filter
      (fun id ->
        let a = Data.artifact resolved id in
        a.producer = None && Hashtbl.mem used id)
      ids
    |> List.sort (fun a b -> natural paths.(Artifact_id.index a) paths.(Artifact_id.index b))
  in
  let targets =
    List.filter
      (fun id -> (Data.artifact resolved id).producer <> None && not (Hashtbl.mem used id))
      ids
  in
  let executables =
    Hashtbl.fold (fun name () acc -> name :: acc) executables [] |> List.sort String.compare
  in
  Ok
    (Object
       [
         ("version", Number 18);
         ("generator", Object [ ("name", Text "spit"); ("version", Text Version.value) ]);
         ("root", option str root);
         ("output_root", option str output_root);
         ("pipeline_folder", Text pipeline_folder);
         ("external_inputs", list (artifact_json resolved paths) external_inputs);
         ("targets", list (artifact_json resolved paths) targets);
         ("executables", strings executables);
         ("tools", List []);
         ("removed", List []);
         ("left_out", List []);
         ("reviews", List []);
         ("pipeline_files", List []);
         ("calls", List []);
         ("jobs", List jobs);
       ])
