type source = { name : string; typ : string; dims : string list; path : string option }

type expect =
  | Jobs of int
  | At of string * string
  | Has of string * string
  | Fails of string list
  | Same of string

type run = Check | Plan of string | Recipe of string

type test = {
  name : string;
  sources : source list;
  steps : string;
  data : (string * string list) list;
  files : string list;
  texts : (string * string) list;
  header : string;
  runs : (run * expect list) list;
  gap : string option;
}

let source ?(typ = "") ?path name dims = { name; typ; dims; path }

let header =
  "default entities: {@key}={@value} separated \"_\"\n\
   default path: {@product}[/{@entities}]\n\
   default ext: .txt\n"

let test ?(sources = []) ?(data = []) ?(datasets = []) ?(files = []) ?(texts = [])
    ?(header = header) ?gap name steps runs =
  let data = if data = [] then datasets else ("data", data) :: datasets in
  { name; sources; steps; data; files; texts; header; runs; gap }

let reads names =
  String.concat ""
    (List.map
       (fun name ->
         Printf.sprintf
           "operation read_%s(x) ->\ncommand read_%s: cp {x} {@output}\nread_%s = read_%s(%s)\n"
           name name name name name)
       names)

let plan expects = (Plan "data", expects)

let plan_in root expects = (Plan root, expects)

let check expects = (Check, expects)

let recipe text expects = (Recipe text, expects)

let jobs n = Jobs n

let at path json = At (path, json)

let fails words = Fails words

let same path = Same path

let has path json = Has (path, json)

let replace before after text =
  let lb = String.length before and lt = String.length text in
  let rec find i =
    if i + lb > lt then
      failwith ("replace: no " ^ String.escaped before ^ " in " ^ String.escaped text)
    else if String.sub text i lb = before then i
    else find (i + 1)
  in
  let i = find 0 in
  String.sub text 0 i ^ after ^ String.sub text (i + lb) (lt - i - lb)

let vary name ?(steps = Fun.id) ?(sources = Fun.id) ?data ?gap runs (base : test) =
  {
    name;
    sources = sources base.sources;
    steps = steps base.steps;
    data = Option.fold ~none:base.data ~some:(fun d -> [ ("data", d) ]) data;
    files = base.files;
    texts = base.texts;
    header = base.header;
    runs;
    gap;
  }

let redim name dims sources =
  List.map (fun (s : source) -> if s.name = name then { s with dims } else s) sources

let operation signature =
  let name = String.trim (List.hd (String.split_on_char '(' signature)) in
  let inside =
    match String.index_opt signature '(' with
    | None -> ""
    | Some i -> String.sub signature (i + 1) (String.index signature ')' - i - 1)
  in
  let ports =
    List.filter_map
      (fun part ->
        match String.split_on_char ':' part with
        | port :: _ when String.trim port <> "" -> Some ("{" ^ String.trim port ^ "}")
        | _ -> None)
      (String.split_on_char ',' inside)
  in
  Printf.sprintf "operation %s\ncommand %s: %s %s {@output}\n" signature name name
    (String.concat " " ports)

let pipeline_text (t : test) =
  let declare (s : source) =
    let typ = if s.typ = "" then "" else s.typ ^ " " in
    let dims = String.concat ", " s.dims in
    let generated =
      s.name ^ "/" ^ String.concat "/" (List.map (fun d -> "{" ^ d ^ "}") s.dims) ^ ".txt"
    in
    let ext = if s.path = None then ".txt " else "" in
    match s.path with
    | Some "" -> Printf.sprintf "source %s: %s[%s]\n" s.name typ dims
    | path ->
        Printf.sprintf "source %s: %s%s[%s]\npath %s: %s\n" s.name typ ext dims s.name
          (Option.value path ~default:generated)
  in
  t.header ^ String.concat "" (List.map declare t.sources) ^ t.steps

let record_path (t : test) record =
  match String.index_opt record '[' with
  | None -> failwith ("record without dimensions: " ^ record)
  | Some open_at ->
      let name = String.sub record 0 open_at in
      let inside = String.sub record (open_at + 1) (String.length record - open_at - 2) in
      let pairs =
        List.map
          (fun pair ->
            match String.split_on_char '=' pair with
            | [ key; value ] -> (key, value)
            | _ -> failwith ("bad record pair: " ^ pair))
          (String.split_on_char ',' inside)
      in
      let declared =
        match List.find_opt (fun (s : source) -> s.name = name) t.sources with
        | Some s -> s.dims
        | None -> failwith ("record of an undeclared source: " ^ record)
      in
      if List.sort compare (List.map fst pairs) <> List.sort compare declared then
        failwith ("record dimensions differ from its source: " ^ record);
      name ^ "/" ^ String.concat "/" (List.map (fun d -> List.assoc d pairs) declared) ^ ".txt"

let rec make_folders path =
  if not (Sys.file_exists path) then (
    make_folders (Filename.dirname path);
    Sys.mkdir path 0o755)

let write path text =
  make_folders (Filename.dirname path);
  let channel = open_out_bin path in
  output_string channel text;
  close_out channel

let rec lookup path (value : Json.t) =
  match (path, value) with
  | [], value -> Some value
  | "#" :: _, Json.List items -> Some (Json.Number (List.length items))
  | "#" :: _, Json.Object fields -> Some (Json.Number (List.length fields))
  | "*" :: rest, Json.List items ->
      let found = List.map (lookup rest) items in
      if List.mem None found then None else Some (Json.List (List.filter_map Fun.id found))
  | key :: rest, Json.List items -> (
      match int_of_string_opt key with
      | Some i when i >= 0 && i < List.length items -> lookup rest (List.nth items i)
      | _ -> None)
  | key :: rest, Json.Object fields -> Option.bind (List.assoc_opt key fields) (lookup rest)
  | _ -> None

let contains text word =
  let lt = String.length text and lw = String.length word in
  let rec at i = i + lw <= lt && (String.sub text i lw = word || at (i + 1)) in
  at 0

let render (e : Data.error) =
  Option.fold ~none:"" ~some:(fun file -> file ^ ": ") e.file ^ e.subject ^ ": " ^ e.message

let outcome run path folder =
  try
    match run with
    | Check -> Result.map (fun _ -> Json.Null) (Driver.check path)
    | Plan root -> Driver.pipeline path (Some (Filename.concat folder root)) None
    | Recipe text ->
        let recipe = Filename.concat folder "recipe.spitin" in
        write recipe text;
        Driver.recipe recipe
  with Sys_error message | Failure message ->
    Error { Data.subject = "exception"; message; span = None; file = None }

let rec judge ~first label result expect =
  match (expect, result) with
  | Fails words, Error e ->
      let text = render e in
      List.filter_map
        (fun word ->
          if contains text word then None
          else Some (Printf.sprintf "%s: the error lacks %S: %s" label word text))
        words
  | Fails _, Ok _ -> [ label ^ ": succeeded, expected an error" ]
  | (Jobs _ | At _ | Has _ | Same _), Error e -> [ label ^ ": " ^ render e ]
  | Has (path, wanted), Ok json -> (
      match lookup (String.split_on_char '.' path) json with
      | Some (Json.List items) when List.exists (fun item -> Json.to_string item = wanted) items ->
          []
      | Some found ->
          [ Printf.sprintf "%s: %s is %s, which lacks %s" label path (Json.to_string found) wanted ]
      | None -> [ Printf.sprintf "%s: %s is missing" label path ])
  | Jobs n, Ok _ -> judge ~first label result (At ("jobs.#", string_of_int n))
  | Same path, Ok json -> (
      let path = String.split_on_char '.' path in
      match Option.map (lookup path) first with
      | Some (Some before) when Some before = lookup path json -> []
      | _ -> [ Printf.sprintf "%s: %s differs from the first run's" label (String.concat "." path) ]
      )
  | At (path, wanted), Ok json -> (
      match lookup (String.split_on_char '.' path) json with
      | Some found when Json.to_string found = wanted -> []
      | Some found ->
          [ Printf.sprintf "%s: %s is %s, expected %s" label path (Json.to_string found) wanted ]
      | None -> [ Printf.sprintf "%s: %s is missing, expected %s" label path wanted ])

let run_test (t : test) =
  let folder = Filename.temp_dir "spit-test-" "" in
  let path = Filename.concat folder "pipeline.spit" in
  write path (pipeline_text t);
  List.iter
    (fun (root, records) ->
      make_folders (Filename.concat folder root);
      List.iter
        (fun r -> write (Filename.concat (Filename.concat folder root) (record_path t r)) "")
        records)
    t.data;
  List.iter (fun file -> write (Filename.concat (Filename.concat folder "data") file) "") t.files;
  List.iter (fun (file, text) -> write (Filename.concat folder file) text) t.texts;
  let first = ref None in
  List.concat_map
    (fun (run, expects) ->
      let label =
        match run with Check -> "check" | Plan root -> "plan " ^ root | Recipe _ -> "recipe"
      in
      let result = outcome run path folder in
      (match (!first, result) with None, Ok json -> first := Some json | _ -> ());
      if expects = [] then match result with Ok _ -> [] | Error e -> [ label ^ ": " ^ render e ]
      else List.concat_map (judge ~first:!first label result) expects)
    t.runs

let main ?(checks = []) groups =
  let gaps = ref 0 in
  let judged group (t : test) =
    let problems = try run_test t with Failure m -> [ "test setup: " ^ m ] in
    match (t.gap, problems) with
    | Some gap, [] -> [ Printf.sprintf "%s/%s: passes, so remove its gap (%s)" group t.name gap ]
    | Some gap, problems ->
        incr gaps;
        if Sys.getenv_opt "SPIT_TEST_GAPS" <> None then
          List.iter
            (fun p -> prerr_endline (Printf.sprintf "gap %s/%s (%s): %s" group t.name gap p))
            problems;
        []
    | None, problems -> List.map (fun p -> Printf.sprintf "%s/%s: %s" group t.name p) problems
  in
  let failures =
    checks @ List.concat_map (fun (group, tests) -> List.concat_map (judged group) tests) groups
  in
  let total = List.fold_left (fun n (_, tests) -> n + List.length tests) 0 groups in
  List.iter prerr_endline failures;
  Printf.printf "Plan tests: %d tests, %d known gaps, %d failures.\n" total !gaps
    (List.length failures);
  if failures <> [] then exit 1
