open Data

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let absolute base path =
  let path = if Filename.is_relative path then Filename.concat base path else path in
  let parts = String.split_on_char '/' path in
  let parts =
    List.fold_left
      (fun acc -> function
        | "" | "." -> acc
        | ".." -> ( match acc with [] -> [] | _ :: rest -> rest)
        | part -> part :: acc)
      [] parts
  in
  "/" ^ String.concat "/" (List.rev parts)

let check path =
  let* pipeline = Loader.load path in
  let result =
    let* expanded = Expand.expand pipeline in
    Check.check expanded
  in
  Result.map_error
    (fun (error : Data.error) ->
      let file =
        Option.bind error.span (fun span ->
            Option.map fst
              (List.find_opt (fun (_, source) -> source == span.Ast.source) pipeline.files))
      in
      { error with file })
    result

let dag pipeline_path inventory_path =
  let* checked = check pipeline_path in
  let* inventory = Inventory.parse (read_file inventory_path) in
  let* resolved = Resolve.resolve checked inventory in
  let* paths = Paths.bind resolved in
  let pipeline_folder = Filename.dirname (absolute (Sys.getcwd ()) pipeline_path) in
  let inventory_folder = Filename.dirname (absolute (Sys.getcwd ()) inventory_path) in
  let root = Option.map (absolute inventory_folder) inventory.root in
  let output_root =
    match (inventory.output, checked.pipeline.syntax.output_dir) with
    | Some output, _ -> Some (absolute inventory_folder output)
    | None, Some output -> Some (absolute pipeline_folder output)
    | _ -> root
  in
  Dag.write resolved paths ~root ~output_root ~pipeline_folder

let directory name given inherited folder =
  match (given, inherited) with
  | Some _, Some _ -> error name "directory is declared both in pipeline and on command line"
  | Some path, None -> Ok (Some (absolute (Sys.getcwd ()) path))
  | None, path -> Ok (Option.map (absolute folder) path)

let discover_dag ?(tools = []) checked root output_root pipeline_folder =
  let* root =
    match root with
    | Some root -> Ok root
    | None -> error "root" "source discovery needs a root directory"
  in
  let* inventory = Discovery.discover ~pipeline_folder checked root in
  let* resolved = Resolve.resolve checked inventory in
  let* paths = Paths.bind resolved in
  Dag.write ~tools resolved paths ~root:(Some root)
    ~output_root:(Some (Option.value output_root ~default:root))
    ~pipeline_folder

let pipeline path root output_root =
  let* checked = check path in
  let folder = Filename.dirname (absolute (Sys.getcwd ()) path) in
  let* root = directory "root" root checked.pipeline.syntax.root folder in
  let* output_root = directory "output" output_root checked.pipeline.syntax.output_dir folder in
  discover_dag checked root output_root folder

let recipe path =
  let* recipe = Recipe.parse (read_file path) in
  let folder = Filename.dirname (absolute (Sys.getcwd ()) path) in
  let pipeline_path = absolute folder recipe.pipeline in
  let* checked = check pipeline_path in
  let pipeline_folder = Filename.dirname pipeline_path in
  let choose name given inherited =
    match (given, inherited) with
    | Some _, Some _ -> error "recipe" (name ^ " is declared in both pipeline and recipe")
    | Some value, None -> Ok (Some (absolute folder value))
    | None, Some value -> Ok (Some (absolute pipeline_folder value))
    | None, None -> Ok None
  in
  let* root = choose "root" recipe.root checked.pipeline.syntax.root in
  let* output = choose "output" recipe.output checked.pipeline.syntax.output_dir in
  let* checked = Recipe.apply_paths checked recipe.paths in
  let* tools =
    match recipe.tools with
    | None -> Ok []
    | Some path ->
        let* tools = Tool_env.parse (read_file (absolute folder path)) in
        let+ () = Tool_env.validate checked.pipeline.syntax.tools tools in
        tools
  in
  discover_dag ~tools checked root output pipeline_folder

let run arguments =
  let* options = Options.parse arguments in
  match options with
  | Options.Help -> Ok (Options.usage, None)
  | Options.Version -> Ok ("spit " ^ Version.value, None)
  | Options.Check path ->
      let+ _ = check path in
      ("checked " ^ path, None)
  | Options.Pipeline (path, root, output_root, output) ->
      let+ dag = pipeline path root output_root in
      (Json.to_string dag, output)
  | Options.Recipe (path, output) ->
      let+ dag = recipe path in
      (Json.to_string dag, output)
  | Options.Dag (pipeline, inventory, output) ->
      let+ dag = dag pipeline inventory in
      (Json.to_string dag, output)

let write (text, output) =
  match output with
  | None | Some "-" -> print_endline text
  | Some path ->
      let channel = open_out_bin path in
      Fun.protect
        ~finally:(fun () -> close_out channel)
        (fun () -> output_string channel (text ^ "\n"))

let () =
  let result =
    try
      let* output = run (List.tl (Array.to_list Sys.argv)) in
      write output;
      Ok ()
    with
    | Sys_error message -> error "file" message
    | Unix.Unix_error (code, operation, path) ->
        error "file" (operation ^ " " ^ path ^ ": " ^ Unix.error_message code)
  in
  match result with
  | Ok () -> ()
  | Error e ->
      let location =
        match e.span with
        | None -> ""
        | Some span ->
            let line, column = Source.position (Source.index span.source) span.start in
            Printf.sprintf "line %d, column %d: " line column
      in
      prerr_endline
        (Option.fold ~none:"" ~some:(fun file -> file ^ ": ") e.file
        ^ location ^ e.subject ^ ": " ^ e.message);
      exit 1
