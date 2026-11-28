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

let distinct errors =
  List.rev
    (List.fold_left
       (fun kept (error : Data.error) ->
         if
           List.exists
             (fun (other : Data.error) -> other.problem = error.problem && other.span = error.span)
             kept
         then kept
         else error :: kept)
       [] errors)

let check path =
  let* pipeline, unresolved = Loader.load path in
  let result =
    let* expanded = one (Expand.expand pipeline) in
    let* checked = Check.check expanded in
    let+ () = Path_check.validate checked in
    checked
  in
  let unresolved_at = List.map (fun (e : Data.error) -> e.span) unresolved in
  let result =
    match (unresolved, result) with
    | [], result -> result
    | _, Ok _ -> Error unresolved
    | _, Error errors ->
        Error
          (unresolved
          @ List.filter (fun (e : Data.error) -> not (List.mem e.span unresolved_at)) errors)
  in
  Result.map_error
    (List.map (fun (error : Data.error) ->
         let file =
           Option.bind error.span (fun span ->
               Option.map fst
                 (List.find_opt (fun (_, source) -> source == span.Ast.source) pipeline.files))
         in
         { error with file }))
    (Result.map_error distinct result)

let dag pipeline_path inventory_path =
  let* checked = check pipeline_path in
  let* inventory = one (Inventory.parse (read_file inventory_path)) in
  let* resolved = one (Resolve.resolve checked inventory) in
  let* paths = one (Paths.bind resolved) in
  let pipeline_folder = Filename.dirname (absolute (Sys.getcwd ()) pipeline_path) in
  let inventory_folder = Filename.dirname (absolute (Sys.getcwd ()) inventory_path) in
  let root = Option.map (absolute inventory_folder) inventory.root in
  let output_root =
    match (inventory.output, checked.pipeline.syntax.output_dir) with
    | Some output, _ -> Some (absolute inventory_folder output)
    | None, Some output -> Some (absolute pipeline_folder output)
    | _ -> root
  in
  one (Dag.write resolved paths ~root ~output_root ~pipeline_folder)

let directory name given inherited folder =
  match (given, inherited) with
  | Some _, Some _ -> error (Problem.Directory_twice name)
  | Some path, None -> Ok (Some (absolute (Sys.getcwd ()) path))
  | None, path -> Ok (Option.map (absolute folder) path)

let discover_dag ?(tools = []) ?(reviews = []) ?contexts ?(rules = Recipe_rules.empty) checked root
    output_root pipeline_folder =
  one
  @@ let* root =
       match root with Some root -> Ok root | None -> error Problem.Discovery_needs_root
     in
     let* inventory = Discovery.discover ~pipeline_folder ?contexts checked root in
     let* inventory, removed = Recipe_rules.apply rules inventory in
     let* resolved = Resolve.resolve checked inventory in
     let found =
       List.filter_map
         (fun (r : Inventory.record) -> Option.map (fun path -> (r.product, r.values, path)) r.path)
         inventory.records
     in
     let* paths = Paths.bind ~found resolved in
     Dag.write ~tools ~reviews ~removed resolved paths ~root:(Some root)
       ~output_root:(Some (Option.value output_root ~default:root))
       ~pipeline_folder

let pipeline path root output_root =
  let* checked = check path in
  let folder = Filename.dirname (absolute (Sys.getcwd ()) path) in
  let* root = one (directory "root" root checked.pipeline.syntax.root folder) in
  let* output_root =
    one (directory "output" output_root checked.pipeline.syntax.output_dir folder)
  in
  discover_dag checked root output_root folder

let recipe path =
  let* recipe = one (Recipe.parse (read_file path)) in
  let folder = Filename.dirname (absolute (Sys.getcwd ()) path) in
  let pipeline_path = absolute folder recipe.pipeline in
  let* checked = check pipeline_path in
  let pipeline_folder = Filename.dirname pipeline_path in
  let choose name given inherited =
    match (given, inherited) with
    | Some _, Some _ -> error (Problem.Pipeline_and_recipe name)
    | Some value, None -> Ok (Some (absolute folder value))
    | None, Some value -> Ok (Some (absolute pipeline_folder value))
    | None, None -> Ok None
  in
  let* root = one (choose "root" recipe.root checked.pipeline.syntax.root) in
  let* output = one (choose "output" recipe.output checked.pipeline.syntax.output_dir) in
  let* checked = one (Recipe.apply_paths checked recipe.paths) in
  let checked = Recipe.apply_default checked recipe.default in
  let* () = Path_check.validate checked in
  let* tools =
    one
      (match recipe.tools with
      | None -> Ok []
      | Some path ->
          let* tools = Tool_env.parse (read_file (absolute folder path)) in
          let+ () = Tool_env.validate checked.pipeline.syntax.tools tools in
          tools)
  in
  let declared =
    List.filter_map
      (function Ast.Review name -> Some name | _ -> None)
      checked.pipeline.syntax.gates
  in
  let* reviews =
    one
      (read_list
         (fun (name, file) ->
           if List.mem name declared then Ok (name, absolute folder file)
           else error (Problem.Recipe_review_unknown name))
         recipe.reviews)
  in
  discover_dag ~tools ~reviews ?contexts:recipe.contexts ~rules:recipe.rules checked root output
    pipeline_folder
