open Ast
open Span

let read_expansion input =
  let invalid () = at input (Invalid_placeholder (text input)) in
  let builtin = starts "@" input in
  let first = if builtin then drop 1 input else input in
  let* () = if empty first || whitespace first.source.[first.start] then invalid () else Ok () in
  let* name, rest = name_prefix (if builtin then drop 1 input else input) in
  let* name =
    if builtin && is_token "here" name then Ok "here" else check_name "a placeholder" name
  in
  if empty rest then Ok (if builtin then Builtin name else Port name)
  else if builtin && starts "(" rest then
    let* args = enclosed '(' ')' rest in
    match (name, args) with
    | ("join" | "list"), [ arg ] ->
        let+ arg = check_name "an input port" arg in
        Function (name, [ arg ])
    | "scalar", arg :: options ->
        let* arg = check_name "an input port" arg in
        let option input =
          let* key, value = separated "=" input in
          let* _ = check_name "a scalar option" key in
          if empty value then at value Empty_value else Ok (text input)
        in
        let+ options = read_list option options in
        Function (name, arg :: options)
    | _ -> invalid ()
  else if starts "$" rest then
    let* property = check_name "an output property" (drop 1 rest) in
    if ((not builtin) || name = "output") && List.mem property [ "dir"; "stem"; "prefix" ] then
      Ok (Property ((if builtin then "@" else "") ^ name, property))
    else invalid ()
  else if (not builtin) && starts "[" rest then
    let* dimensions = enclosed '[' ']' rest in
    match dimensions with
    | [ dim ] ->
        let+ dimension = check_name "a dimension" dim in
        Dimension (name, dimension)
    | _ -> invalid ()
  else invalid ()

type scanner = {
  input : span;
  buffer : Buffer.t;
  mutable cursor : int;
  mutable quote : (char * int) option;
  mutable problem : diagnostic option;
  mutable commands : command list;
  mutable words : command_word list;
  mutable parts : command_part list;
  mutable word_start : int option;
  mutable text_start : int;
  mutable quoted : bool;
  mutable escaped : bool;
}

let begin_word state =
  if state.word_start = None then (
    state.word_start <- Some state.cursor;
    state.text_start <- state.cursor)

let literal state c =
  begin_word state;
  Buffer.add_char state.buffer c;
  state.cursor <- state.cursor + 1

let flush_text state =
  if Buffer.length state.buffer > 0 then (
    state.parts <-
      Text
        {
          span = slice state.text_start state.cursor state.input;
          value = Buffer.contents state.buffer;
        }
      :: state.parts;
    Buffer.clear state.buffer)

let word_role state span parts =
  if state.quoted || state.escaped then Argument
  else
    match parts with
    | [ Text t ] -> (
        match t.value with
        | "<" -> Input_redirect
        | ">" -> Output_redirect
        | "|" | "||" | "&&" | ";" | ">>" | "&" ->
            state.problem <- Some { span; problem = Shell_operator };
            Argument
        | _ -> Argument)
    | _ -> Argument

let flush_word state =
  match state.word_start with
  | None -> ()
  | Some start ->
      flush_text state;
      let span = slice start state.cursor state.input in
      let parts =
        match List.rev state.parts with [] -> [ Text { span; value = "" } ] | parts -> parts
      in
      let role = word_role state span parts in
      state.words <- { span; parts; quoted = state.quoted; role } :: state.words;
      state.word_start <- None;
      state.parts <- [];
      state.quoted <- false;
      state.escaped <- false

let flush_command state =
  flush_word state;
  match List.rev state.words with
  | [] -> ()
  | first :: rest ->
      let finish =
        List.fold_left (fun _ (w : command_word) -> w.span.finish) first.span.finish rest
      in
      state.commands <-
        { span = slice first.span.start finish state.input; words = first :: rest }
        :: state.commands;
      state.words <- []

let escape state =
  let input = state.input in
  if state.cursor + 1 = input.finish then
    state.problem <-
      Some
        {
          span = slice state.cursor (state.cursor + 1) input;
          problem = Unexpected_text "unfinished command continuation";
        }
  else
    let next = input.source.[state.cursor + 1] in
    if next = '\n' then state.cursor <- state.cursor + 2
    else if next = '\r' && state.cursor + 2 < input.finish && input.source.[state.cursor + 2] = '\n'
    then state.cursor <- state.cursor + 3
    else if state.quote = None || List.mem next [ '"'; '\\'; '$'; '`' ] then (
      begin_word state;
      state.escaped <- true;
      Buffer.add_char state.buffer next;
      state.cursor <- state.cursor + 2)
    else literal state '\\'

let expansion state =
  begin_word state;
  flush_text state;
  let input = state.input in
  let parsed =
    let* inside, rest = read_until 0 [ "}" ] (slice (state.cursor + 1) input.finish input) in
    if empty rest then at (slice state.cursor (state.cursor + 1) input) Unclosed_bracket
    else
      let+ value = read_expansion inside in
      (value, rest.start + 1)
  in
  match parsed with
  | Error d -> state.problem <- Some d
  | Ok (value, finish) ->
      state.parts <- Expand { span = slice state.cursor finish input; value } :: state.parts;
      state.cursor <- finish;
      state.text_start <- finish

let function_expansion state =
  begin_word state;
  flush_text state;
  let input = slice state.cursor state.input.finish state.input in
  let parsed =
    let* _, arguments = read_until 0 [ "(" ] input in
    let* _, rest = read_delimited 0 '(' ')' arguments in
    let* value = read_expansion (slice input.start rest.start input) in
    Ok (value, rest.start)
  in
  match parsed with
  | Error problem -> state.problem <- Some problem
  | Ok (value, finish) ->
      state.parts <- Expand { span = slice input.start finish input; value } :: state.parts;
      state.cursor <- finish;
      state.text_start <- finish

let starts_expansion state =
  let input = state.input and i = state.cursor in
  (i = input.start || input.source.[i - 1] <> '$')
  && i + 1 < input.finish
  && (name_char input.source.[i + 1] || input.source.[i + 1] = '@')

let ordinary state c =
  if c = '\n' && state.quote = None then (
    flush_command state;
    state.cursor <- state.cursor + 1)
  else if whitespace c && state.quote = None then (
    flush_word state;
    state.cursor <- state.cursor + 1)
  else if c = '#' && state.quote = None && state.word_start = None then
    while state.cursor < state.input.finish && state.input.source.[state.cursor] <> '\n' do
      state.cursor <- state.cursor + 1
    done
  else if c = '{' && starts_expansion state then expansion state
  else if
    c = '@'
    && List.exists
         (fun name -> starts_at ("@" ^ name ^ "(") state.cursor state.input)
         [ "scalar"; "join"; "list" ]
  then if state.quote = None then function_expansion state else literal state c
  else literal state c

let scan_character state =
  let c = state.input.source.[state.cursor] in
  match state.quote with
  | Some ('\'', _) ->
      if c = '\'' then (
        state.quote <- None;
        state.cursor <- state.cursor + 1)
      else literal state c
  | _ -> (
      match c with
      | '\'' when state.quote = None ->
          begin_word state;
          state.quoted <- true;
          state.quote <- Some (c, state.cursor);
          state.cursor <- state.cursor + 1
      | '"' ->
          begin_word state;
          state.quoted <- true;
          state.quote <- (if state.quote = None then Some (c, state.cursor) else None);
          state.cursor <- state.cursor + 1
      | '\\' -> escape state
      | _ -> ordinary state c)

let script ~indent input =
  let state =
    {
      input;
      buffer = Buffer.create 32;
      cursor = input.start;
      quote = None;
      problem = None;
      commands = [];
      words = [];
      parts = [];
      word_start = None;
      text_start = input.start;
      quoted = false;
      escaped = false;
    }
  in
  while state.cursor < input.finish && state.problem = None do
    scan_character state
  done;
  match (state.problem, state.quote) with
  | Some d, _ -> Error d
  | None, Some (c, start) -> at (slice start (start + 1) input) (Unclosed_quote c)
  | None, None -> (
      flush_command state;
      match state.problem with
      | Some d -> Error d
      | None -> Ok { span = input; indent; commands = List.rev state.commands })

let parse input =
  let+ script = script ~indent:0 input in
  List.concat_map
    (fun (command : command) ->
      List.concat_map
        (fun (word : command_word) ->
          List.map (function Text t -> Literal t.span | Expand e -> Expansion e) word.parts)
        command.words)
    script.commands
