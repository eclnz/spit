let read channel =
  let buffer = Buffer.create 4096 and bytes = Bytes.create 4096 in
  let rec loop () =
    match input channel bytes 0 4096 with
    | 0 -> Buffer.contents buffer
    | n ->
        Buffer.add_subbytes buffer bytes 0 n;
        loop ()
  in
  loop ()

let run_file path =
  let source =
    if path = "-" then read stdin
    else
      let channel = open_in_bin path in
      Fun.protect ~finally:(fun () -> close_in channel) (fun () -> read channel)
  in
  let pipeline, diagnostics = Parser.parse source in
  print_endline
    (Json.to_string
       (Json.Object
          [
            ("file", Json.Text path); ("result", Syntax_json.result ~source (pipeline, diagnostics));
          ]));
  diagnostics = []

let () =
  let files =
    match Array.to_list Sys.argv |> List.tl with
    | [] -> [ "v1_mock.spit"; "v2_mock.spit"; "v3_mock.spit"; "v4_mock.spit"; "stats_mock.spit" ]
    | [ "--stdin" ] -> [ "-" ]
    | files -> files
  in
  let success =
    List.fold_left
      (fun ok file ->
        try
          let valid = run_file file in
          valid && ok
        with Sys_error message ->
          prerr_endline message;
          false)
      true files
  in
  if not success then exit 1
