open Ast
open Span

let read ~entities input =
  let cursor = ref input.start in
  let invalid start message = at (slice start !cursor input) (Invalid_pattern message) in
  let plain span =
    match (text span, entities) with
    | "@key", true -> Ok Entity_key
    | "@value", true -> Ok Entity_value
    | "@product", false -> Ok Product_name
    | "@stage", false -> Ok Stage_name
    | "@entities", false -> Ok Entity_names
    | name, false when not (String.starts_with ~prefix:"@" name) ->
        let+ name = check_name "a dimension" span in
        Dimension_name (name, None)
    | name, false -> at span (Unknown_builtin name)
    | (("key" | "value") as name), true -> at span (Builtin_needs_at name)
    | name, true -> at span (Entities_placeholder name)
  in
  let property span dollar =
    let raw = text span in
    let name = slice span.start (span.start + dollar) span in
    let* name = check_name "an artifact" name in
    match String.sub raw (dollar + 1) (String.length raw - dollar - 1) with
    | "dir" -> Ok (Artifact_property (name, Dir))
    | "stem" -> Ok (Artifact_property (name, Stem))
    | _ -> at span (Path_property_unknown raw)
  in
  let value span =
    let raw = text span in
    match (String.index_opt raw '$', String.index_opt raw ':') with
    | Some dollar, _ when not entities -> property span dollar
    | _, None -> plain span
    | _, Some colon -> (
        let name = slice span.start (span.start + colon) span in
        let shape = String.sub raw (colon + 1) (String.length raw - colon - 1) in
        let* parsed = plain name in
        match (parsed, Shape.of_name shape) with
        | Dimension_name (name, None), Some shape -> Ok (Dimension_name (name, Some shape))
        | Dimension_name _, None -> at span (Unknown_shape { shape; placeholder = raw })
        | _ -> at span Builtin_shape)
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
              if List.exists (function Pattern_value _ -> true | _ -> false) parts then
                loop (Pattern_optional parts :: acc)
              else at (slice start !cursor input) Group_without_placeholder
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
      let* pattern = read ~entities:true pattern in
      let values =
        List.filter_map
          (function Pattern_value { value; _ } -> Some value | _ -> None)
          pattern.segments
      in
      let adjacent =
        let rec walk = function
          | Pattern_value _ :: Pattern_value _ :: _ -> true
          | _ :: rest -> walk rest
          | [] -> false
        in
        walk pattern.segments
      in
      if values <> [ Entity_key; Entity_value ] then at pattern.span Entities_order
      else if adjacent then at pattern.span Entities_adjacent
      else if separator = "" || String.contains separator '/' || String.contains separator '%' then
        at rest Entities_separator
      else Ok { span = input; pattern; separator }
  | _ -> at rest Entities_one_separator
