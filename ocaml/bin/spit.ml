open Data

let run arguments =
  let* options = Options.parse arguments in
  match options with
  | Options.Help -> Ok (Options.usage, None)
  | Options.Version -> Ok ("spit " ^ Version.value, None)
  | Options.Check path ->
      let+ _ = Driver.check path in
      ("checked " ^ path, None)
  | Options.Pipeline (path, root, output_root, output) ->
      let+ dag = Driver.pipeline path root output_root in
      (Json.to_string dag, output)
  | Options.Recipe (path, output) ->
      let+ dag = Driver.recipe path in
      (Json.to_string dag, output)
  | Options.Dag (pipeline, inventory, output) ->
      let+ dag = Driver.dag pipeline inventory in
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
    | Sys_error message -> error (Problem.File_failed message)
    | Unix.Unix_error (code, operation, path) ->
        error (Problem.File_failed (operation ^ " " ^ path ^ ": " ^ Unix.error_message code))
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
        ^ location ^ Problem.render e.problem);
      exit 1
