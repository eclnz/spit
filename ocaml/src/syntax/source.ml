open Ast

let index text =
  let starts = ref [ 0 ] in
  String.iteri (fun i c -> if c = '\n' then starts := (i + 1) :: !starts) text;
  Array.of_list (List.rev !starts)

let position starts offset =
  let first = ref 0 and last = ref (Array.length starts) in
  while !first + 1 < !last do
    let middle = (!first + !last) / 2 in
    if starts.(middle) <= offset then first := middle else last := middle
  done;
  (!first + 1, offset - starts.(!first) + 1)

let script_lines (span : span) indent =
  if span.start = span.finish then []
  else
    String.sub span.source span.start (span.finish - span.start)
    |> String.split_on_char '\n'
    |> List.map (fun line ->
        if String.trim line = "" then ""
        else
          let finish = String.length line - if String.ends_with ~suffix:"\r" line then 1 else 0 in
          String.sub line indent (finish - indent))
