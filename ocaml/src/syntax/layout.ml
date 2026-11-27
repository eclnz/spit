open Ast
open Span

let lines source =
  let n = String.length source in
  let rec loop start acc =
    if start > n then List.rev acc
    else
      let finish = Option.value (String.index_from_opt source start '\n') ~default:n in
      let span = { source; start; finish } in
      let trimmed = trim span in
      let indent = if empty trimmed then finish - start else trimmed.start - start in
      loop (finish + 1) ({ indent; span } :: acc)
  in
  loop 0 []

let ignored (line : line) =
  let span = trim line.span in
  empty span || starts "#" span

let rec drop_while predicate = function
  | x :: xs when predicate x -> drop_while predicate xs
  | rest -> rest

let take_body raw indent (pending : line list) =
  let rec take acc (pending : line list) =
    match pending with
    | line :: rest
      when empty (trim line.span) || ((not raw) && ignored line) || line.indent > indent ->
        take (line :: acc) rest
    | rest -> (List.rev acc, rest)
  in
  take [] pending

let script (body : line list) fallback =
  let body = List.rev (drop_while (fun (l : line) -> empty (trim l.span)) (List.rev body)) in
  match body with
  | [] -> Script_body (slice fallback.finish fallback.finish fallback, 0)
  | first :: _ ->
      let finish = (List.hd (List.rev body)).span.finish in
      let indent =
        List.fold_left
          (fun n (l : line) -> if empty (trim l.span) then n else min n l.indent)
          max_int body
      in
      Script_body (slice first.span.start finish first.span, indent)

let takes_script header =
  match header.rule.form with
  | Script -> true
  | Gate (Check | Screen) -> not header.default
  | _ -> false

type entry_start = Default_block | Keyword_block of entry_header | Keyword_line of entry_header

let classify_header inherited_default rule rest =
  let header = { rule; default = inherited_default; rest } in
  if (not (is_token ":" rest)) || inherited_default then Ok (Keyword_line header)
  else if rule.keyword = Keyword.Stage then at rest Stage_needs_name
  else Ok (Keyword_block header)

let read_header inherited_default supplied input =
  let rec read depth inherited_default supplied input =
    if depth > max_nesting then at input Nesting_limit
    else
      match supplied with
      | Some rule -> classify_header inherited_default rule input
      | None -> (
          let* word, rest = read_until 0 [ " "; "\t"; ":" ] input in
          let rest = trim rest in
          match text word with
          | "default" when is_token ":" rest -> Ok Default_block
          | "default" -> read (depth + 1) true None rest
          | word -> (
              match keyword word with
              | Some rule -> classify_header inherited_default rule rest
              | None when String.contains (text input) '=' ->
                  at input (Missing_binding_keyword (text input))
              | None -> at input (Unknown_keyword (text input))))
  in
  read 0 inherited_default supplied input

let recover_header limit (header : entry_header) (line : line) pending =
  let pending =
    match read_until 0 [ "\r"; "\n" ] { header.rest with finish = limit } with
    | Ok (span, _) -> drop_while (fun (line : line) -> line.span.start < span.finish) pending
    | Error _ -> pending
  in
  let _, remaining = take_body false line.indent pending in
  match remaining with
  | next :: rest
    when let value = trim next.span in
         starts ">" value || starts ")" value || starts "]" value ->
      rest
  | _ -> remaining

let read_entry read_syntax limit header (line : line) pending =
  let raw = takes_script header in
  let joined =
    if raw then
      let+ head, value = split_entry header.rest in
      {
        syntax = Plain_header;
        head;
        value;
        rest = slice header.rest.finish header.rest.finish header.rest;
      }
    else read_syntax header.rule.keyword { header.rest with finish = limit }
  in
  match joined with
  | Error problem -> (Error problem, recover_header limit header line pending)
  | Ok parsed ->
      let span = trim (slice header.rest.start parsed.rest.start header.rest) in
      let pending = drop_while (fun (line : line) -> line.span.start < span.finish) pending in
      let body, remaining = take_body raw line.indent pending in
      let entry =
        let head, value = (parsed.head, parsed.value) in
        let+ content =
          match (raw, value) with
          | true, Some value when empty value -> Ok (script body value)
          | true, Some value when List.for_all ignored body -> Ok (Script_body (value, 0))
          | true, Some value -> at value (Inline_script_with_body (text head, text value))
          | true, None -> at head (Missing_script (header.rule.word, text head))
          | false, Some value when empty value -> Ok (Body body)
          | false, _ when List.exists (fun l -> not (ignored l)) body ->
              at span (Unexpected_indent (text span))
          | false, Some value -> Ok (Value value)
          | false, None -> Ok Bare
        in
        { header; head; content; syntax = parsed.syntax }
      in
      (entry, remaining)

let entries read_syntax supplied inherited_default pending =
  let rec group depth supplied inherited_default pending =
    let limit = match List.rev pending with [] -> 0 | (l : line) :: _ -> l.span.finish in
    let rec loop acc pending =
      match drop_while ignored pending with
      | [] -> List.rev acc
      | line :: rest ->
          let body () = take_body false line.indent rest in
          let values, remaining =
            if depth > max_nesting then
              let _, remaining = body () in
              ([ at line.span Nesting_limit ], remaining)
            else
              match read_header inherited_default supplied (trim line.span) with
              | Error e ->
                  let _, remaining = body () in
                  ([ Error e ], remaining)
              | Ok Default_block ->
                  let body, remaining = body () in
                  (group (depth + 1) None true body, remaining)
              | Ok (Keyword_block header) ->
                  let body, remaining = body () in
                  (group (depth + 1) (Some header.rule) header.default body, remaining)
              | Ok (Keyword_line header) ->
                  let entry, remaining = read_entry read_syntax limit header line rest in
                  ([ entry ], remaining)
          in
          loop (List.rev_append values acc) remaining
    in
    loop [] pending
  in
  group 0 supplied inherited_default pending

let single (e : entry) =
  let candidates =
    match e.content with
    | Value value -> [ value ]
    | Body lines ->
        List.filter_map (fun (l : line) -> if ignored l then None else Some (trim l.span)) lines
    | _ -> []
  in
  match candidates with
  | [ value ] -> Ok value
  | values -> at e.head (Value_count (text e.head, List.length values))

let script_lines (e : entry) =
  match e.content with
  | Script_body (span, indent) -> Source.script_lines span indent
  | Value v -> [ text v ]
  | Body lines ->
      List.filter_map
        (fun (l : line) -> if ignored l then None else Some (text (trim l.span)))
        lines
  | _ -> []

let content_text (e : entry) = String.concat " " (script_lines e)

let owner (e : entry) = "`" ^ e.header.rule.word ^ "`"

let target every named (e : entry) =
  match (e.header.default, empty e.head) with
  | true, true -> Ok every
  | true, false -> at e.head (Named_default (text e.head))
  | false, _ when is_token "default" e.head ->
      at e.head (Default_target (e.header.rule.word, content_text e))
  | false, _ ->
      let+ name = check_name (owner e) e.head in
      named name

let operation_target = target Every_operation (fun s -> Named_operation s)

let artifact_target = target Every_artifact (fun s -> Named_artifact s)

let dimension_target = target Every_dimension (fun s -> Named_dimension s)

let value_only (e : entry) =
  match (empty e.head, e.content) with
  | true, Value v -> Ok v
  | _ -> at e.head (Expected_value_only (e.header.rule.word, text e.header.rest))

let name_only (e : entry) =
  match (empty e.head, e.content) with
  | false, Bare -> Ok e.head
  | false, Value v -> at v (Expected_name_only (e.header.rule.word, text v))
  | _ -> at e.head (Expected_name e.header.rule.word)
