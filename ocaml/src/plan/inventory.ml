open Ast
open Data

type record = { product : string; values : (string * string) list; path : string option }

type t = {
  root : string option;
  output : string option;
  records : record list;
  contexts : (string * string) list list;
}

type reading = {
  mutable section : bool;
  mutable root : string option;
  mutable output : string option;
}

let syntax subject result =
  Result.map_error
    (fun (d : diagnostic) ->
      { file = None; span = Some d.span; problem = Problem.Syntax (subject, d.problem) })
    result

let dimensions rest =
  let open Span in
  if empty rest then Ok []
  else
    let* dims = enclosed '[' ']' rest in
    read_list
      (fun dim ->
        let* key, value = separated "=" dim in
        let* key = check_name "a dimension" key in
        if empty value then at value Empty_value else Ok (key, text value))
      dims

let identity input =
  let* name, rest = syntax "inventory" (Span.read_until 0 [ "[" ] input) in
  let* name = syntax "inventory" (Span.check_name "a source" name) in
  let+ values = syntax name (dimensions rest) in
  { product = name; values; path = None }

let root_directive state input text =
  let is_root = Span.starts "root " input in
  let existing = if is_root then state.root else state.output in
  if existing <> None then error Problem.Inventory_root_twice
  else
    let skip = if is_root then 5 else 7 in
    let value = String.trim (String.sub text skip (String.length text - skip)) in
    if value = "" then error Problem.Inventory_root_empty
    else (
      if is_root then state.root <- Some value else state.output <- Some value;
      Ok None)

let read_line state (line : line) =
  let input = Span.trim line.span in
  let text = Span.text input in
  if Layout.ignored line then Ok None
  else if text = "sources:" && line.indent = 0 then (
    state.section <- true;
    Ok None)
  else if (not state.section) && (Span.starts "root " input || Span.starts "output " input) then
    root_directive state input text
  else if state.section && line.indent > 0 then
    let+ record = identity input in
    Some record
  else error Problem.Inventory_unsupported

let parse source =
  let state = { section = false; root = None; output = None } in
  let+ records = read_list (read_line state) (Layout.lines source) in
  {
    root = state.root;
    output = state.output;
    records = List.filter_map Fun.id records;
    contexts = [];
  }
