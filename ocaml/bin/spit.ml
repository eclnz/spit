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

let run = function
  | [ "check"; path ] ->
      let+ _ = check path in
      "checked " ^ path
  | [ "dag"; pipeline; inventory ] ->
      let+ dag = dag pipeline inventory in
      Json.to_string dag
  | _ -> error "usage" "ocaml/spit.sh check PIPELINE | dag PIPELINE INVENTORY"

let () =
  let result =
    try run (List.tl (Array.to_list Sys.argv)) with Sys_error message -> error "file" message
  in
  match result with
  | Ok text -> print_endline text
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
