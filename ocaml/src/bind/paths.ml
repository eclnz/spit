open Ast
open Data

type rule = {
  product : product;
  pattern : pattern;
  extension : extension option;
  format : entity_format option;
  keys : (string, string) Hashtbl.t;
}

let prepare product generated =
  let specific = ref None and default = ref None and extension = ref None and format = ref None in
  let keys = Hashtbl.create 8 in
  List.iter
    (fun (s : Setting.t located) ->
      match s.value with
      | Path (Named_artifact name, pattern) when name = product.name -> specific := Some pattern
      | Path (Every_artifact, pattern) -> default := Some pattern
      | Ext (Every_artifact, ext) -> extension := Some ext
      | Entities (Every_dimension, Format value) -> format := Some value
      | Entities (Named_dimension dim, Rename name) -> Hashtbl.replace keys dim name
      | _ -> ())
    product.settings;
  let* pattern =
    match (!specific, !default) with
    | Some pattern, _ | None, Some pattern -> Ok pattern
    | _ -> error product.name "artifact has no path rule"
  in
  let declared = match product.shape with File ext -> ext | _ -> None in
  let extension =
    match (declared, !specific, generated) with
    | Some ext, _, _ -> Some ext
    | None, Some _, _ | None, None, false -> None
    | None, None, true -> !extension
  in
  Ok { product; pattern; extension; format = !format; keys }

let encode source =
  let buffer = Buffer.create (String.length source) in
  String.iter
    (function
      | ('a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '-' | '_' | '.') as c -> Buffer.add_char buffer c
      | c -> Buffer.add_string buffer (Printf.sprintf "%%%02X" (Char.code c)))
    source;
  Buffer.contents buffer

let rec render_parts value parts =
  let buffer = Buffer.create 64 and missing = ref None in
  let* _ =
    read_list
      (function
        | Pattern_text text ->
            Buffer.add_string buffer text.value;
            Ok ()
        | Pattern_value part -> (
            let+ found = value part.value in
            match found with
            | Some text -> Buffer.add_string buffer text
            | None -> missing := Some part.span)
        | Pattern_optional parts ->
            let+ text, absent = render_parts value parts in
            if absent = None then Buffer.add_string buffer text)
      parts
  in
  Ok (Buffer.contents buffer, !missing)

let entities_format rule symbols entities =
  match rule.format with
  | None when Array.length entities = 0 -> Ok ""
  | None ->
      error ~span:rule.pattern.span rule.product.name "{@entities} needs a default entities format"
  | Some format ->
      let* parts =
        read_list
          (fun (dim, value) ->
            let dim = symbol_text symbols dim in
            let key = Option.value (Hashtbl.find_opt rule.keys dim) ~default:dim in
            let+ text, _ =
              render_parts
                (function
                  | Entity_key -> Ok (Some key)
                  | Entity_value -> Ok (Some (encode (symbol_text symbols value)))
                  | _ -> error ~span:format.span rule.product.name "invalid entity format")
                format.pattern.segments
            in
            text)
          (Array.to_list entities)
      in
      Ok (String.concat format.separator parts)

let render rule symbols entities =
  let value = function
    | Product_name -> Ok (Some rule.product.path_name)
    | Stage_name ->
        Ok (if rule.product.stage = [] then None else Some (String.concat "/" rule.product.stage))
    | Entity_names ->
        let+ text = entities_format rule symbols entities in
        if text = "" then None else Some text
    | Dimension_name name ->
        Ok
          (Option.bind (Hashtbl.find_opt symbols.ids name) (fun dim ->
               Option.map
                 (fun (_, value) -> encode (symbol_text symbols value))
                 (Array.find_opt (fun (d, _) -> d = dim) entities)))
    | Entity_key | Entity_value ->
        error ~span:rule.pattern.span rule.product.name "invalid path placeholder"
  in
  let* path, missing = render_parts value rule.pattern.segments in
  match missing with
  | Some span -> error ~span rule.product.name "a required path placeholder has no value"
  | None -> (
      if
        path = ""
        || (not (Filename.is_relative path))
        || List.mem ".." (String.split_on_char '/' path)
      then error ~span:rule.pattern.span rule.product.name "artifact path must stay within its root"
      else
        match rule.extension with
        | None -> Ok path
        | Some (Append _) ->
            error rule.product.name
              "appended extensions are not implemented in the compiler experiment"
        | Some (Replace ext) -> (
            match rule.pattern.suffix with
            | None -> Ok (path ^ ext)
            | Some literal when literal = ext -> Ok path
            | Some _ ->
                error ~span:rule.pattern.span rule.product.name
                  "path extension conflicts with declared extension"))

let bind resolved =
  let used = Hashtbl.create (Array.length resolved.artifacts) in
  let rules = Array.make (Array.length resolved.checked.products) None in
  let bind_artifact (artifact : artifact) =
    let index = Product_id.index artifact.product in
    let* rule =
      match rules.(index) with
      | Some rule -> Ok rule
      | None ->
          let+ rule =
            prepare (product resolved.checked artifact.product) (artifact.producer <> None)
          in
          rules.(index) <- Some rule;
          rule
    in
    let* path = render rule resolved.checked.symbols artifact.entities in
    if Hashtbl.mem used path then error rule.product.name ("artifact path collision: " ^ path)
    else (
      Hashtbl.add used path ();
      Ok path)
  in
  let+ paths = read_list bind_artifact (Array.to_list resolved.artifacts) in
  Array.of_list paths
