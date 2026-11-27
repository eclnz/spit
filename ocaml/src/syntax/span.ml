open Ast

let ( let* ) = Result.bind

let ( let+ ) value f = Result.map f value

let read_list read values =
  let rec loop acc = function
    | [] -> Ok (List.rev acc)
    | first :: rest ->
        let* value = read first in
        loop (value :: acc) rest
  in
  loop [] values

let text (s : span) = String.sub s.source s.start (s.finish - s.start)

let empty (s : span) = s.start = s.finish

let slice start finish (s : span) = { s with start; finish }

let drop count (s : span) = { s with start = s.start + count }

let starts_at token offset (s : span) =
  let n = String.length token in
  if offset + n > s.finish then false
  else
    let matched = ref 0 in
    while !matched < n && s.source.[offset + !matched] = token.[!matched] do
      incr matched
    done;
    !matched = n

let starts token (s : span) = starts_at token s.start s

let is_token token (s : span) = s.finish - s.start = String.length token && starts token s

let max_nesting = 128

let whitespace = function ' ' | '\t' | '\r' | '\n' | '\012' -> true | _ -> false

let letter = function 'a' .. 'z' | 'A' .. 'Z' -> true | _ -> false

let upper = function 'A' .. 'Z' -> true | _ -> false

let name_char = function 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '_' -> true | _ -> false

let trim (s : span) =
  let a = ref s.start and b = ref s.finish in
  while !a < !b && whitespace s.source.[!a] do
    incr a
  done;
  while !b > !a && whitespace s.source.[!b - 1] do
    decr b
  done;
  slice !a !b s

let at span problem = Error { span; problem }

let rec read_until depth stops input =
  let cursor = ref input.start and problem = ref None in
  let first = ref input.finish and finish = ref input.start in
  while
    !cursor < input.finish && !problem = None
    && not (List.exists (fun token -> starts_at token !cursor input) stops)
  do
    let begin_at = !cursor in
    let c = input.source.[!cursor] in
    (match c with
    | '"' | '\'' ->
        incr cursor;
        while !cursor < input.finish && input.source.[!cursor] <> c do
          cursor :=
            !cursor + if input.source.[!cursor] = '\\' && !cursor + 1 < input.finish then 2 else 1
        done;
        if !cursor = input.finish then
          problem := Some { span = slice begin_at (begin_at + 1) input; problem = Unclosed_quote c }
        else incr cursor
    | '(' | '[' -> (
        match
          read_delimited (depth + 1) c
            (if c = '(' then ')' else ']')
            (slice !cursor input.finish input)
        with
        | Ok (_, rest) -> cursor := rest.start
        | Error e -> problem := Some e)
    | ')' | ']' ->
        problem := Some { span = slice !cursor (!cursor + 1) input; problem = Unexpected_closing c }
    | '#' ->
        let before = ref (!cursor - 1) in
        while !before >= 0 && input.source.[!before] <> '\n' && whitespace input.source.[!before] do
          decr before
        done;
        if !before >= 0 && input.source.[!before] <> '\n' then
          problem := Some { span = slice !cursor (!cursor + 1) input; problem = Inline_comment }
        else
          while !cursor < input.finish && input.source.[!cursor] <> '\n' do
            incr cursor
          done
    | _ -> incr cursor);
    if c <> '#' && not (whitespace c) then (
      first := min !first begin_at;
      finish := !cursor)
  done;
  match !problem with
  | Some e -> Error e
  | None ->
      let consumed =
        if !finish < !first then slice !cursor !cursor input else slice !first !finish input
      in
      Ok (consumed, slice !cursor input.finish input)

and read_delimited depth opening closing input =
  if depth > max_nesting then at input Nesting_limit
  else if not (starts (String.make 1 opening) input) then
    at input (Missing_delimiters (opening, closing, text input))
  else
    let* inside, rest = read_until depth [ String.make 1 closing ] (drop 1 input) in
    if empty rest then at (slice input.start (input.start + 1) input) Unclosed_bracket
    else Ok (inside, drop 1 rest)

let words input =
  let rec read acc remaining =
    let remaining = trim remaining in
    if empty remaining then Ok (List.rev acc)
    else
      let* word, rest = read_until 0 [ " "; "\t"; "\r"; "\n" ] remaining in
      read (if empty word then acc else word :: acc) rest
  in
  read [] input

let require_end rest = if empty (trim rest) then Ok () else at rest (Unexpected_text (text rest))

let separated separator input =
  let* left, rest = read_until 0 [ separator ] input in
  if empty rest then at input (Missing_separator (separator, text input))
  else Ok (left, trim (drop (String.length separator) rest))

let split_entry input =
  let+ head, rest = read_until 0 [ ":" ] input in
  (head, if empty rest then None else Some (trim (drop 1 rest)))

let reserved = [ "here"; "beside"; "suffix"; "from"; "view"; "default"; "many" ]

let check_name owner span =
  let name = text span in
  if name = "" then at span (Missing_name owner)
  else if name = "default" then at span Default_used_as_name
  else if List.mem name reserved then at span (Reserved_name name)
  else if not ((letter name.[0] || name.[0] = '_') && String.for_all name_char name) then
    at span (Invalid_name name)
  else Ok name

let read_ref span =
  let dot = ref span.start in
  while !dot < span.finish && span.source.[!dot] <> '.' do
    incr dot
  done;
  match !dot with
  | dot when dot < span.finish ->
      let* namespace = check_name "a namespace" (slice span.start dot span) in
      let+ name = check_name "a reference" (slice (dot + 1) span.finish span) in
      { namespace = Some namespace; name }
  | _ ->
      let+ name = check_name "a reference" span in
      { namespace = None; name }

let rec trivia input =
  let input = trim input in
  if starts "#" input then
    let* _, rest = read_until 0 [ "\n" ] input in
    trivia rest
  else Ok input

let name_prefix input =
  let+ input = trivia input in
  let finish = ref input.start in
  while !finish < input.finish && name_char input.source.[!finish] do
    incr finish
  done;
  (slice input.start !finish input, slice !finish input.finish input)

let comma_items closing reader input =
  let ended rest = match closing with Some t -> starts t rest | None -> empty rest in
  let finish rest = match closing with Some t -> drop (String.length t) rest | None -> rest in
  let rec read required acc remaining =
    let* remaining = trivia remaining in
    if ended remaining then
      if required then at remaining Empty_list_item else Ok (List.rev acc, finish remaining)
    else if empty remaining then at remaining Unclosed_bracket
    else
      let* value, rest = reader remaining in
      if rest.start = remaining.start then at rest Empty_list_item
      else
        let* rest = trivia rest in
        if ended rest then Ok (List.rev (value :: acc), finish rest)
        else if starts "," rest then read true (value :: acc) (drop 1 rest)
        else if empty rest then at rest Unclosed_bracket
        else at rest (Unexpected_text (text rest))
  in
  read false [] input

let comma_list input =
  let+ values, _ = comma_items None (read_until 0 [ "," ]) input in
  values

let applied input =
  let* name, rest = read_until 0 [ "(" ] input in
  if empty rest then Ok (name, [])
  else
    let* inside, rest = read_delimited 0 '(' ')' rest in
    let* () = require_end rest in
    let+ args = comma_list inside in
    (name, args)

let enclosed opening closing input =
  let* inside, rest = read_delimited 0 opening closing input in
  let* () = require_end rest in
  comma_list inside

let horizontal input =
  let start = ref input.start in
  while !start < input.finish && (input.source.[!start] = ' ' || input.source.[!start] = '\t') do
    incr start
  done;
  { input with start = !start }
