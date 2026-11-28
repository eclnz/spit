open Ast
open Span

let read ~entities input =
  let cursor = ref input.start in
  let invalid start message = at (slice start !cursor input) (Invalid_pattern message) in
  let value span =
    match (text span, entities) with
    | "@key", true -> Ok Entity_key
    | "@value", true -> Ok Entity_value
    | "@product", false -> Ok Product_name
    | "@stage", false -> Ok Stage_name
    | "@entities", false -> Ok Entity_names
    | name, false when not (String.starts_with ~prefix:"@" name) ->
        let+ name = check_name "a dimension" span in
        Dimension_name name
    | name, _ -> at span (Invalid_pattern ("unknown placeholder `" ^ name ^ "`"))
  in
  let rec section depth optional =
    if depth > max_nesting then at input Nesting_limit
    else
      let rec loop acc =
        if !cursor = input.finish then
          if optional then invalid input.start "unclosed optional group" else Ok (List.rev acc)
        else
          let start = !cursor in
          match input.source.[!cursor] with
          | ']' ->
              incr cursor;
              if optional then Ok (List.rev acc) else invalid start "unexpected closing group"
          | '[' ->
              incr cursor;
              let* parts = section (depth + 1) true in
              loop (Pattern_optional parts :: acc)
          | '{' -> (
              incr cursor;
              match String.index_from_opt input.source !cursor '}' with
              | Some finish when finish < input.finish ->
                  let location = slice !cursor finish input in
                  cursor := finish + 1;
                  let* parsed = value location in
                  loop (Pattern_value { span = location; value = parsed } :: acc)
              | _ -> invalid start "unclosed placeholder")
          | '}' ->
              incr cursor;
              invalid start "unexpected closing placeholder"
          | _ ->
              while
                !cursor < input.finish
                && not (List.mem input.source.[!cursor] [ '['; ']'; '{'; '}' ])
              do
                incr cursor
              done;
              let span = slice start !cursor input in
              loop (Pattern_text { span; value = text span } :: acc)
      in
      loop []
  in
  let+ parts = section 0 false in
  let raw = text input in
  let start = match String.rindex_opt raw '}' with None -> 0 | Some i -> i + 1 in
  let tail = String.sub raw start (String.length raw - start) in
  let suffix =
    Option.map
      (fun i -> String.sub tail i (String.length tail - i) |> String.split_on_char ']' |> List.hd)
      (String.index_opt tail '.')
  in
  { span = input; segments = parts; suffix }

let path input = read ~entities:false input

let entities input =
  let* pattern, rest = separated " separated " input in
  let* separator = words rest in
  match separator with
  | [ separator ] ->
      let raw = text separator in
      let separator =
        if String.length raw >= 2 && raw.[0] = '"' && raw.[String.length raw - 1] = '"' then
          String.sub raw 1 (String.length raw - 2)
        else raw
      in
      let+ pattern = read ~entities:true pattern in
      { span = input; pattern; separator }
  | _ -> at rest (Invalid_pattern "entities need one separator")
