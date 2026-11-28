open Ast
open Data
open Json

let typ =
  let rec encode = function
    | Var v -> Object [ ("variable", Text v) ]
    | Named (n, args) -> Object [ ("name", Text (ref_name n)); ("args", list encode args) ]
  in
  encode

let lookup json key = match json with Object fields -> List.assoc_opt key fields | _ -> None

let relative_member paths view member =
  let view_path = paths.(Artifact_id.index view) ^ "/" in
  let path = paths.(Artifact_id.index member) in
  if String.starts_with ~prefix:view_path path then
    Some (String.sub path (String.length view_path) (String.length path - String.length view_path))
  else None

let artifact_json with_ids resolved paths id =
  let artifact = Data.artifact resolved id in
  let view = artifact.members <> [||] in
  let product = Data.product resolved.checked artifact.product in
  Object
    ([
       ("product", Text product.name);
       ( "entities",
         Object
           (Array.to_list artifact.entities
           |> List.map (fun (d, v) ->
               ( symbol_text resolved.checked.symbols d,
                 Text (symbol_text resolved.checked.symbols v) ))) );
       ("type", option typ product.typ);
       ("path", Text paths.(Artifact_id.index id));
       ( "under",
         Text
           (if product.here then "pipeline_folder"
            else if artifact.producer = None && not view then "root"
            else "output_root") );
     ]
    @ (if with_ids then [ ("id", Number (Artifact_id.index id)) ] else [])
    @ [
        ( "kind",
          Text
            (match product.shape with
            | File _ -> "file"
            | Folder | Folder_from _ -> "folder"
            | View -> "directory_view") );
      ]
    @
    if view then
      [
        ( "members",
          list
            (fun member ->
              Object
                [
                  ("id", Number (Artifact_id.index member));
                  ("path", Text (Option.value (relative_member paths id member) ~default:""));
                ])
            (Array.to_list artifact.members) );
      ]
    else [])

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

let job_json with_ids resolved paths dependents (job : job) commands verify checks screens tools =
  let step = resolved.checked.steps.(job.step) in
  let params = Array.of_list step.operation.params
  and outputs = Array.of_list step.operation.outputs in
  let inputs =
    Array.to_list
      (Array.mapi
         (fun i ids ->
           let value = list (artifact_json with_ids resolved paths) (Array.to_list ids) in
           (params.(i).name, value))
         job.inputs)
  in
  let output_fields =
    Array.to_list
      (Array.mapi
         (fun i id -> (port_name outputs.(i), artifact_json with_ids resolved paths id))
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
       ("checks", list Artifact_check.json checks);
       ("cores", Number step.configuration.cores);
       ("memory", option (fun n -> Number n) step.configuration.memory);
       ("tools", strings tools);
       ("anchored", Bool step.configuration.anchored);
     ]
    @ collection_json resolved step params
    @ (if screens = [] then [] else [ ("screens", list Artifact_check.screen_json screens) ])
    @
    match
      List.filter_map
        (fun (o : output) ->
          match o.shape with Folder_from port -> Some (port_name o, Text port) | _ -> None)
        step.operation.outputs
    with
    | [] -> []
    | seeds -> [ ("seed_folders", Object seeds) ])

let executable executables arguments =
  match arguments with
  | [ Text name ] :: _ when not (String.contains name '/') -> Hashtbl.replace executables name ()
  | _ -> ()

let bind_job with_ids tools resolved paths dependents used executables (job : job) =
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
  let* () = if commands = [] then error (Problem.No_command step.operation.name) else Ok () in
  List.iter (fun (arguments, _, _) -> executable executables arguments) (commands @ verify);
  let* checks = Artifact_check.bind resolved paths job in
  List.iter (fun (check : Artifact_check.bound) -> executable executables check.command) checks;
  let* screens = Artifact_check.screens resolved paths job in
  List.iter
    (fun ((screen : Artifact_check.bound), _) -> executable executables screen.command)
    screens;
  Array.iter (fun ids -> Array.iter (fun id -> Hashtbl.replace used id ()) ids) job.inputs;
  Ok
    (job_json with_ids resolved paths dependents job commands verify checks screens
       (Tool_env.used tools step.operation.name step.settings))

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

let review_targets resolved name =
  let found = ref [] in
  let add target = if not (List.mem target !found) then found := target :: !found in
  let reviewed settings applies =
    List.exists
      (fun (used : gate_use) -> ref_name used.gate = name)
      (Artifact_check.uses ~kind:Gate_kind.Review settings applies)
  in
  Array.iter
    (fun (job : job) ->
      let step = resolved.checked.steps.(job.step) in
      let operation = step.operation in
      List.iteri
        (fun port ((param : param), ids) ->
          Array.iter
            (fun id ->
              let product = Data.product resolved.checked (Data.artifact resolved id).product in
              if
                reviewed product.settings (function
                  | Named_artifact target -> target = product.name
                  | _ -> false)
              then add (id, None);
              if
                reviewed step.settings (function
                  | Input_of (target, input) -> target = operation.name && input = param.name
                  | _ -> false)
              then add (id, Some (job.id, port)))
            ids)
        (List.combine operation.params (Array.to_list job.inputs));
      List.iter
        (fun ((output : output), id) ->
          let product = Data.product resolved.checked (Data.artifact resolved id).product in
          if
            reviewed product.settings (function
              | Every_artifact -> true
              | Named_artifact target -> target = product.name
              | Output_of (target, port) -> target = operation.name && port = output.name
              | _ -> false)
          then add (id, None))
        (List.combine operation.outputs (Array.to_list job.outputs)))
    resolved.jobs;
  List.rev !found

let review_json with_ids resolved paths files name =
  let targets = review_targets resolved name in
  let dims id = (Data.product resolved.checked (Data.artifact resolved id).product).dims in
  let dimensions =
    match targets with
    | [] -> []
    | (first, _) :: rest ->
        List.filter
          (fun dim -> List.for_all (fun (id, _) -> Array.mem dim (dims id)) rest)
          (Array.to_list (dims first))
  in
  Object
    [
      ("name", Text name);
      ("file", option str (List.assoc_opt name files));
      ("dimensions", strings (List.map (symbol_text resolved.checked.symbols) dimensions));
      ( "targets",
        list
          (fun (id, input) ->
            Object
              [
                ("artifact", artifact_json with_ids resolved paths id);
                ( "input",
                  option
                    (fun (job, port) ->
                      Object [ ("job", Number (Job_id.number job)); ("port", Number port) ])
                    input );
              ])
          targets );
    ]

let write ?(tools = []) ?(reviews = []) ?(removed = []) resolved paths ~root ~output_root
    ~pipeline_folder =
  let* () =
    if tools = [] then Ok () else Tool_env.validate resolved.checked.pipeline.syntax.tools tools
  in
  let tool_names = List.map (fun (tool : Tool_env.tool) -> tool.name) tools in
  let dependents = dependents resolved in
  let used = Hashtbl.create 64 and executables = Hashtbl.create 8 in
  let views =
    List.filter
      (fun id -> (Data.artifact resolved id).members <> [||])
      (List.init (Array.length resolved.artifacts) (fun i -> Artifact_id.Artifact_id i))
  in
  let with_ids = views <> [] in
  let name id = (Data.product resolved.checked (Data.artifact resolved id).product).name in
  let all = List.init (Array.length resolved.artifacts) (fun i -> Artifact_id.Artifact_id i) in
  let folders =
    List.filter
      (fun id ->
        let artifact = Data.artifact resolved id in
        artifact.producer <> None
        &&
        match (Data.product resolved.checked artifact.product).shape with
        | Folder | Folder_from _ -> true
        | File _ | View -> false)
      all
  in
  let* _ =
    read_list
      (fun folder ->
        let prefix = paths.(Artifact_id.index folder) ^ "/" in
        match
          List.find_opt
            (fun id -> id <> folder && String.starts_with ~prefix paths.(Artifact_id.index id))
            all
        with
        | Some inside ->
            error
              (Problem.Inside_output_folder
                 {
                   product = name inside;
                   folder = name folder;
                   path = paths.(Artifact_id.index inside);
                 })
        | None -> Ok ())
      folders
  in
  let* _ =
    read_list
      (fun view ->
        read_list
          (fun member ->
            Hashtbl.replace used member ();
            match relative_member paths view member with
            | Some _ -> Ok ()
            | None ->
                error
                  (Problem.View_member_outside
                     {
                       view =
                         (Data.product resolved.checked (Data.artifact resolved view).product).name;
                       member =
                         (Data.product resolved.checked (Data.artifact resolved member).product)
                           .name;
                       path = paths.(Artifact_id.index member);
                     }))
          (Array.to_list (Data.artifact resolved view).members))
      views
  in
  let* jobs =
    read_list
      (bind_job with_ids tool_names resolved paths dependents used executables)
      (Array.to_list resolved.jobs)
  in
  let ids = List.init (Array.length resolved.artifacts) (fun i -> Artifact_id.Artifact_id i) in
  let external_inputs =
    List.filter
      (fun id ->
        let a = Data.artifact resolved id in
        a.producer = None && a.members = [||] && Hashtbl.mem used id)
      ids
    |> List.sort (fun a b -> natural paths.(Artifact_id.index a) paths.(Artifact_id.index b))
  in
  let targets =
    List.filter
      (fun id -> (Data.artifact resolved id).producer <> None && not (Hashtbl.mem used id))
      ids
    @ List.filter (fun id -> not (Hashtbl.mem used id)) views
  in
  List.iter
    (fun (tool : Tool_env.tool) ->
      Option.iter
        (fun probe -> executable executables (List.map (fun argument -> [ Text argument ]) probe))
        tool.probe)
    tools;
  let executables =
    Hashtbl.fold (fun name () acc -> name :: acc) executables [] |> List.sort String.compare
  in
  let screened =
    List.concat_map
      (fun job ->
        match lookup job "screens" with
        | Some (List screens) ->
            List.filter_map
              (fun screen ->
                match lookup screen "path" with Some (Text path) -> Some path | _ -> None)
              screens
        | _ -> [])
      jobs
  in
  let screen_products =
    List.filter_map
      (fun id ->
        if List.mem paths.(Artifact_id.index id) screened then
          Some (Data.product resolved.checked (Data.artifact resolved id).product)
        else None)
      ids
    |> List.sort_uniq (fun (a : product) b -> String.compare a.name b.name)
  in
  let* screen_records =
    read_list
      (fun (product : product) ->
        let record = "screens/" ^ product.path_name ^ ".csv" in
        if Array.mem record paths then
          error (Problem.Path_collision { product = product.name; path = record })
        else Ok (product.name, Text record))
      screen_products
  in
  Ok
    (Object
       [
         ("version", Number 18);
         ("generator", Object [ ("name", Text "spit"); ("version", Text Version.value) ]);
         ("root", option str root);
         ("output_root", option str output_root);
         ("pipeline_folder", Text pipeline_folder);
         ("external_inputs", list (artifact_json with_ids resolved paths) external_inputs);
         ("targets", list (artifact_json with_ids resolved paths) targets);
         ("executables", strings executables);
         ("tools", list Tool_env.json tools);
         ("removed", List removed);
         ("screen_records", Object screen_records);
         ("left_out", List []);
         ( "reviews",
           list
             (review_json with_ids resolved paths reviews)
             (List.filter_map
                (function Review name -> Some name | _ -> None)
                resolved.checked.pipeline.syntax.gates) );
         ( "pipeline_files",
           list
             (fun (path, contents) ->
               Object
                 [
                   ("path", Text (Blob.relative ~folder:pipeline_folder path));
                   ("blob", Text (Blob.id contents));
                 ])
             resolved.checked.pipeline.files );
         ("calls", List []);
         ("jobs", List jobs);
       ])
