open Ast
open Span
open Parser

let span source = { source; start = 0; finish = String.length source }

let success = function
  | Ok value -> value
  | Error d -> failwith (Diagnostics.render_problem d.problem)

let failure = function Error _ -> () | Ok _ -> failwith "expected a diagnostic"

let valid source =
  let pipeline, diagnostics = parse source in
  if diagnostics <> [] then
    failwith (Json.to_string (Json.list (Syntax_json.diagnostic (Source.index source)) diagnostics));
  pipeline

let invalid source =
  let _, diagnostics = parse source in
  if diagnostics = [] then failwith ("expected diagnostics: " ^ source);
  diagnostics

let artifact_names scope = List.map (fun (a : artifact) -> a.name) scope.artifacts

let check condition message = if not condition then failwith message

let read_file path =
  let channel = open_in_bin path in
  Fun.protect
    ~finally:(fun () -> close_in channel)
    (fun () -> really_input_string channel (in_channel_length channel))

let diagnostic_line (d : diagnostic) =
  fst (Source.position (Source.index d.span.source) d.span.start)

let test_delimiters_and_recovery () =
  let inside, remaining = success (read_delimited 0 '(' ')' (span "(x)[y]")) in
  check (text inside = "x" && text remaining = "[y]") "delimiter remainder";
  List.iter (fun s -> check (success (comma_list (span s)) = []) "empty list") [ ""; " "; "\n" ];
  List.iter (fun s -> failure (comma_list (span s))) [ ","; ",x"; "x,"; "x,,y"; "x, ,y" ];
  check
    (List.map text (success (comma_list (span "\"x,y\", g(a,b)"))) = [ "\"x,y\""; "g(a,b)" ])
    "nested commas";
  List.iter (fun s -> failure (read_delimited 0 '(' ')' (span s))) [ "([x))"; "(x]"; "([x)"; "(x" ];
  let source = "product malformed = f([x))\nsource kept: Mean\n" in
  let pipeline, diagnostics = parse source in
  check (artifact_names pipeline.global = [ "kept" ]) "recovery after brackets";
  (match diagnostics with
  | [ { span = location; problem = Unexpected_closing ')' } ] ->
      check
        (text location = ")"
        && location.start = String.index source ')'
        && location.source == source)
        "original source location"
  | _ -> failwith "unexpected bracket diagnostics");
  List.iter
    (fun expression ->
      let p, ds = parse ("product bad = " ^ expression ^ "\nsource kept: Mean") in
      check
        (List.length ds = 1 && artifact_names p.global = [ "kept" ])
        "malformed argument recovery")
    [ "f(x,,y)"; "f(,)"; "f(x,)"; "f(,x)" ];
  List.iter (fun e -> ignore (valid ("product accepted = " ^ e))) [ "f()"; "f(x)"; "f(x, y)" ];
  ignore (invalid "product alias = existing");
  ignore (invalid "product missing = ");
  ignore (invalid "product a = f(x) trailing");
  ignore (invalid "product a = f(a<b)")

let test_types_and_annotations () =
  let p = valid "operation tested(x: Mean @check(test(\"a->b\"))) -> Mean" in
  check
    (List.exists
       (fun (s : Setting.t located) ->
         match s.value with
         | Setting.Gate (_, Input_of ("tested", "x"), [ gate ]) -> gate.args = [ "\"a->b\"" ]
         | _ -> false)
       p.global.settings)
    "quoted arrow";
  List.iter
    (fun value -> ignore (valid ("source raw: Mean @check(compare(" ^ value ^ "))")))
    [ "a<b"; "A<B" ];
  ignore (valid "calculate lower: left < right\npath raw: folder<name/file");
  let source =
    "operation tested(\n    x: Mean,\n    y: Mean,,\n    z: Mean\n) -> Mean\nsource kept: Mean"
  in
  let p, ds = parse source in
  check (List.length ds = 1 && diagnostic_line (List.hd ds) = 3) "multiline empty item";
  check (artifact_names p.global = [ "kept" ]) "multiline recovery";
  List.iter
    (fun (source, line, fragment) ->
      match invalid source with
      | [ d ] -> check (diagnostic_line d = line && text d.span = fragment) "annotation span"
      | _ -> failwith "annotation diagnostic count")
    [
      ("operation convert(\n    input: Mean @check()\n) -> Mean", 2, "@check()");
      ("operation convert(\n    input: Mean @check(a)\n    @check(b)\n) -> Mean", 3, "@check(b)");
      ("type Pair<\n    $A,\n    Mean\n>", 3, "Mean");
    ];
  let p =
    valid
      "type Pair<\n    $Left,\n    $Right\n>\nsource raw: Pair<\n    Mean,\n    Box<Mean>\n> .csv"
  in
  check
    (List.map (fun (t : type_def) -> (t.name, t.params)) p.type_defs
    = [ ("Pair", [ "Left"; "Right" ]) ])
    "generic declaration";
  ignore (valid "operation f(x:\n# type comment\n Pair<Mean, Box<Mean>>) -> Pair<Mean, Mean>");
  List.iter (fun t -> ignore (invalid ("source malformed: " ^ t))) [ "Mean>"; "Mean.csv" ];
  failure (read_ref (span "one.two.three"));
  check
    (success (read_ref (span "one.two")) = { namespace = Some "one"; name = "two" })
    "qualified reference";
  check (success (check_name "an artifact" (span "_name")) = "_name") "underscore name";
  ignore
    (valid "source plain\nsource folder: Index / [subject]\nsource sidecar: Text +.txt beside plain");
  let p =
    valid "source raw: stats.Readings<Box<$Kind>> .csv\noperation f(x: stats.Mean) -> stats.Mean"
  in
  (match (List.hd p.global.artifacts).origin with
  | Found { typ = Some (Named ({ namespace = Some "stats"; name = "Readings" }, [ _ ])); _ } -> ()
  | _ -> failwith "qualified generic type");
  ignore (valid "source raw: stats.Pair<\n    stats.Mean,\n    Box<Mean>\n> .csv")

let test_scripts_and_jobs () =
  let p = valid "command script:\n    # first\n    echo   hi\n\n    # last\nsource kept: Mean" in
  check
    (List.filter_map
       (fun (s : Setting.t located) ->
         match s.value with
         | Setting.Command (Named_operation "script", script) ->
             Some (Syntax_json.script_lines script)
         | _ -> None)
       p.global.settings
    = [ [ "# first"; "echo   hi"; ""; "# last" ] ])
    "lossless script";
  let p =
    valid
      "operation split(x: Mean @check(okay)) -> (left: Mean @check(okay), right: Mean)\n\
       product a, b = split(input)\n\
       product c = split(input)"
  in
  check
    (List.map
       (fun (a : artifact) ->
         match a.origin with Made (job, index, _) -> (job, index) | _ -> (-1, -1))
       p.global.artifacts
    = [ (0, 0); (0, 1); (1, 0) ])
    "job and output identity";
  check (List.length p.global.jobs = 2 && List.length p.global.settings = 2) "single call per job"

let test_scope_recovery () =
  let source = "stage scope:\n    product answer = f(\narg\n)\nsource kept: Mean" in
  let p, ds = parse source in
  check
    ((List.hd p.global.stages).scope.artifacts = [] && artifact_names p.global = [ "kept" ])
    "scope boundary";
  check (List.exists (fun d -> d.problem = Unclosed_bracket) ds) "unclosed stage call";
  check
    (List.length (List.filter (fun d -> diagnostic_line d = 3) ds) = 1)
    "single dedented diagnostic";
  let p = valid "stage scope:\n    product answer = f(\n        arg\n    )\nsource kept: Mean" in
  check (artifact_names (List.hd p.global.stages).scope = [ "answer" ]) "stage continuation";
  let p, ds =
    parse "unknown first\nstage scope:\n    unknown child\n    product kept = f()\nunknown last"
  in
  check (List.map diagnostic_line ds = [ 1; 3; 5 ]) "diagnostic order";
  check (artifact_names (List.hd p.global.stages).scope = [ "kept" ]) "partial scope";
  let deep =
    "operation deep(x: "
    ^ String.concat "" (List.init 150 (fun _ -> "Box<"))
    ^ "Mean" ^ String.make 150 '>' ^ ") -> Mean\nsource kept: Mean"
  in
  let p, ds = parse deep in
  check
    (List.map (fun d -> d.problem) ds = [ Nesting_limit ] && artifact_names p.global = [ "kept" ])
    "type nesting bound";
  let stages =
    List.init 141 (fun depth -> String.make (depth * 4) ' ' ^ "stage s" ^ string_of_int depth ^ ":")
  in
  let p, ds = parse (String.concat "\n" stages ^ "\nsource kept: Mean") in
  check
    (List.map (fun d -> d.problem) ds = [ Nesting_limit ] && artifact_names p.global = [ "kept" ])
    "scope nesting bound"

let test_defaults_and_gates () =
  let p =
    valid
      "default check: nonempty\n\
       stage s:\n\
      \    default check: !nonempty\n\
      \    product a = f()\n\
       stage s:\n\
      \    product b, c = g()"
  in
  let s = (List.hd p.global.stages).scope in
  check (List.length p.global.stages = 1 && artifact_names s = [ "a"; "b"; "c" ]) "repeated stages";
  check
    (List.map (fun (a : artifact) -> a.origin) s.artifacts
    = [ Made (0, 0, None); Made (1, 0, None); Made (1, 1, None) ])
    "merged stage job ids";
  check
    (List.exists
       (fun (s : Setting.t located) ->
         match s.value with
         | Setting.Gate (_, Every_artifact, [ { negated = true; _ } ]) -> true
         | _ -> false)
       s.settings)
    "stage gate removal";
  ignore (valid "default check .nii.gz: nifti_ok\nstage s:\n    default check .nii.gz: !nifti_ok");
  List.iter
    (fun source -> ignore (invalid source))
    [
      "default command: cp {input} {@output}";
      "default:\n    command: echo hello";
      "default stage s:";
      "default source: Mean";
      "stage s:\n    source raw: Mean";
      "root: one\nroot: two";
      "types: strange";
      "default ext: csv";
      "default path: one\ndefault path: two";
      "stage s:\n    default ext: .csv\nstage s:\n    default ext: .txt";
      "operation f(x) -> (y: Mean):\n    source raw: Mean";
      "operation f(x) -> Mean:\n    product y = g(x)";
      "operation f(x) -> (y: Mean .csv):\n    product y = g(x)";
    ];
  ignore
    (valid
       "default:\n\
       \    verify:\n\
       \        test -n {input}\n\
       \    resources: cores=2\n\
       \    uses: awk\n\
       \    anchored");
  let p =
    valid
      "product collected = f(raw @where(acq=fast) @vary(run))\n\
       product first = f(raw @where(acq=fast) @first(session))\n\
       product last = f(raw @last(session))"
  in
  check
    (List.map
       (fun (i : iteration located) -> i.value)
       (List.hd (List.hd p.global.jobs).args).iterations
    = [ Where [ ("acq", "fast") ]; Vary [ "run" ] ])
    "selector composition";
  List.iter
    (fun s -> ignore (invalid s))
    [
      "product a = f(raw @vary())";
      "product a = f(raw @vary(x) @vary(y))";
      "product a = f(raw @where(x=))";
      "product a = f(raw @same(x) @first(y))";
    ]

let test_template_syntax () =
  let source = "source raw: Mean @check(nonempty)" in
  let parsed = read_scope 0 File_scope (Layout.lines source) in
  (match parsed.entries with
  | [ Ok { value = Source_declaration { gates = [ annotation ]; _ }; _ } ] ->
      check (text annotation.span = "@check(nonempty)") "gate source span"
  | _ -> failwith "parsed gate attachment");
  let pieces =
    success
      (Template.parse
         (span
            "x {report$dir} {report$stem} {report$prefix} {means[site]} {@output$dir} \
             {@scalar(offset, min=1)} {@join(means)} {@list(means)}"))
  in
  let expansions = List.filter_map (function Expansion x -> Some x.value | _ -> None) pieces in
  check
    (expansions
    = [
        Property ("report", "dir");
        Property ("report", "stem");
        Property ("report", "prefix");
        Dimension ("means", "site");
        Property ("@output", "dir");
        Function ("scalar", [ "offset"; "min=1" ]);
        Function ("join", [ "means" ]);
        Function ("list", [ "means" ]);
      ])
    "template syntax";
  let scalar_parts = success (Template.parse (span "n=@scalar(size, min=1, max=10)")) in
  let scalars = List.filter_map (function Expansion x -> Some x.value | _ -> None) scalar_parts in
  check (scalars = [ Function ("scalar", [ "size"; "min=1"; "max=10" ]) ]) "bare scalar";
  let collections = success (Template.parse (span "@join(means) @list(means)")) in
  let functions =
    List.filter_map (function Expansion x -> Some x.value | _ -> None) collections
  in
  check
    (functions = [ Function ("join", [ "means" ]); Function ("list", [ "means" ]) ])
    "collection functions";
  let quoted = success (Template.parse (span "'@scalar(size)' \"@scalar(size)\"")) in
  check (List.for_all (function Expansion _ -> false | _ -> true) quoted) "quoted scalar text";
  ignore (valid "command f: echo '{report.dir} @join(xs)' \"{@join(xs)}\" ${shell} {@here}");
  List.iter
    (fun s -> ignore (invalid ("command f: " ^ s)))
    [
      "echo {report.dir}";
      "echo {means.site}";
      "echo {@ join(means)}";
      "echo {report$unknown}";
      "echo {means[]}";
      "echo {@join()}";
      "echo {unclosed";
    ];
  let p =
    valid "operation outer(x: Mean) -> (y: Mean):\n    product mid = f(x)\n    product y = g(mid)"
  in
  check (Option.map List.length (List.hd p.global.operations).body = Some 2) "composite bindings";
  let json = Json.to_string (Json.Text "a  b\n\t\"\\") in
  check (json = "\"a  b\\n\\t\\\"\\\\\"") "JSON preserves whitespace"

let test_flat_scale_and_crlf () =
  let large =
    String.concat "\n" (List.init 100000 (fun i -> "product p" ^ string_of_int i ^ " = f()"))
  in
  let p = valid large in
  check
    (List.length p.global.jobs = 100000 && List.length p.global.artifacts = 100000)
    "large flat pipeline";
  let crlf = valid "command f:\r\n    echo {input}  \r\nsource raw: Mean\r\n" in
  check
    (List.exists
       (fun (s : Setting.t located) ->
         match s.value with
         | Setting.Command (Named_operation "f", script) ->
             Syntax_json.script_lines script = [ "echo {input}  " ]
         | _ -> false)
       crlf.global.settings)
    "CRLF and trailing script whitespace"

let test_stage_scale_and_flags () =
  List.iter
    (fun source -> ignore (invalid source))
    [
      "default anchored: false";
      "default anchored:\n    product lost = f()";
      "source raw: ..";
      "source raw: ..csv";
      "source raw: .csv.";
      "default ext: .a..b";
    ];
  ignore (valid "default anchored");
  ignore (valid "source raw: .nii.gz\ndefault ext: .tar.gz");
  let reopened =
    String.concat "\n"
      (List.init 10000 (fun i -> "stage s:\n    product p" ^ string_of_int i ^ " = f()"))
  in
  let repeated = valid reopened in
  let merged = (List.hd repeated.global.stages).scope in
  check
    (List.length repeated.global.stages = 1 && List.length merged.jobs = 10000)
    "linear reopened stages";
  List.iteri
    (fun i (a : artifact) -> check (a.origin = Made (i, 0, None)) "rebased stage job")
    merged.artifacts;
  let wide =
    valid
      (String.concat "\n"
         (List.init 10000 (fun i ->
              "stage s" ^ string_of_int i ^ ":\n    product p" ^ string_of_int i ^ " = f()")))
  in
  check (List.length wide.global.stages = 10000) "wide stages"

let test_located_commands () =
  let located_source =
    "operation convert(\n\
    \    input: Mean @check(okay)\n\
     ) -> Mean\n\
     product answer = convert(\n\
    \    missing @where(site=dune) @vary(day)\n\
     )"
  in
  let locations = valid located_source in
  let call = List.hd locations.global.jobs in
  let argument = List.hd call.args in
  check
    (text call.callee_span = "convert" && text argument.artifact_span = "missing")
    "retained reference spans";
  check
    (List.map (fun (i : iteration located) -> text i.span) argument.iterations
    = [ "@where(site=dune)"; "@vary(day)" ])
    "retained selector spans";
  check
    (text (List.hd locations.global.settings).span = "@check(okay)")
    "retained normalized gate span";
  check
    ((List.hd locations.global.operations).span.source == located_source)
    "shared declaration source";
  ignore (invalid "command f: echo \"unterminated");
  let command_source =
    "command f:\n\
    \    echo \"before {input} after\" \\\n\
    \    {@output}\n\
    \    # literal comment\n\
    \    echo 'literal {input}'"
  in
  let commands = valid command_source in
  match (List.hd commands.global.settings).value with
  | Setting.Command (_, script) ->
      check (List.length script.commands = 2) "command AST logical lines";
      let command = List.hd script.commands in
      check (List.length command.words = 3) "command AST word boundaries";
      let expanded =
        List.concat_map
          (fun (word : command_word) ->
            List.filter_map (function Expand e -> Some e | _ -> None) word.parts)
          command.words
      in
      check
        (List.map (fun (e : expansion located) -> text e.span) expanded = [ "{input}"; "{@output}" ])
        "command expansion source spans"
  | _ -> failwith "expected parsed command AST"

let test_command_roles () =
  let words source =
    let script = success (Template.script ~indent:0 (span source)) in
    (List.hd script.commands).words
  in
  let quoted_operators = words {|printf %s \> \< \| \&\& '>' "&&" > {@output}|} in
  check
    (List.map (fun (word : command_word) -> word.role) quoted_operators
    = [
        Argument;
        Argument;
        Argument;
        Argument;
        Argument;
        Argument;
        Argument;
        Argument;
        Output_redirect;
        Argument;
      ])
    "escaped and quoted operators are arguments";
  List.iter
    (fun operator -> failure (Template.script ~indent:0 (span ("echo " ^ operator))))
    [ "|"; "||"; "&&"; ";"; ">>"; "&" ];
  check
    (List.map (fun (word : command_word) -> word.role) (words "echo >\\\n {@output}")
    = [ Argument; Output_redirect; Argument ])
    "redirect role survives continuation"

let test_source_and_patterns () =
  let source = "one\r\ntwo\n" in
  let starts = Source.index source in
  check
    (List.map (Source.position starts) [ 0; 3; 5; 8; 9 ]
    = [ (1, 1); (1, 4); (2, 1); (2, 4); (3, 1) ])
    "indexed CRLF and end-of-file locations";
  let pattern_source = "path data:\n    base[/{subject}[/{run}]]/{@product}\n" in
  let p = valid pattern_source in
  (match (List.hd p.global.settings).value with
  | Setting.Path (_, pattern) ->
      check
        (pattern.span.source == pattern_source
        && text pattern.span = "base[/{subject}[/{run}]]/{@product}")
        "path pattern retains original source";
      check
        (List.exists (function Pattern_optional _ -> true | _ -> false) pattern.segments)
        "structured optional path group"
  | _ -> failwith "expected path pattern");
  List.iter
    (fun source -> ignore (invalid source))
    [
      "path raw: {@unknown}";
      "default entities: {@product} separated \"_\"";
      "default entities: {@key}";
      "path raw: unclosed/{subject";
    ];
  let a = valid "source raw: Mean .csv [subject]"
  and b = valid "source raw:\n    Mean .csv [subject]" in
  check
    (Syntax_json.pipeline a = Syntax_json.pipeline b)
    "inline and body source share description grammar"

let test_membership_scale () =
  let gates = String.concat "," (List.init 10000 (fun i -> "g" ^ string_of_int i)) in
  ignore (valid ("source raw: Mean @check(" ^ gates ^ ")"));
  ignore (invalid "source raw: Mean @check(g, !g)");
  let defaults =
    String.concat "\n" (List.init 10000 (fun i -> "default check .x" ^ string_of_int i ^ ": okay"))
  in
  ignore (valid defaults);
  check
    (List.length (invalid (defaults ^ "\ndefault check .x9999: okay")) = 1)
    "large default duplicate check"

let test_malformed_inputs () =
  let state = Random.State.make [| 42 |] in
  let alphabet = "abc @():[],=<>#$'\"\n " in
  for _ = 1 to 2000 do
    let source =
      String.init (Random.State.int state 180) (fun _ ->
          alphabet.[Random.State.int state (String.length alphabet)])
    in
    let _, ds = parse source in
    List.iter
      (fun (d : diagnostic) ->
        check
          (d.span.start >= 0
          && d.span.finish <= String.length source
          && d.span.start <= d.span.finish)
          "bounded error span")
      ds
  done

let () =
  let directory = Sys.argv.(1) in
  List.iter
    (fun file -> ignore (valid (read_file (Filename.concat directory file))))
    [ "v1_mock.spit"; "v2_mock.spit"; "v3_mock.spit"; "v4_mock.spit"; "stats_mock.spit" ];
  test_delimiters_and_recovery ();
  test_types_and_annotations ();
  test_scripts_and_jobs ();
  test_scope_recovery ();
  test_defaults_and_gates ();
  test_template_syntax ();
  test_flat_scale_and_crlf ();
  test_stage_scale_and_flags ();
  test_located_commands ();
  test_command_roles ();
  test_source_and_patterns ();
  test_membership_scale ();
  test_malformed_inputs ();
  print_endline
    "OCaml parser checks passed (five mocks, grammar, scopes, templates, recovery and 2,000 \
     malformed inputs)."
