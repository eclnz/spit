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
    | _ -> error (Problem.No_path_rule product.name)
  in
  let declared = match product.shape with File ext -> ext | _ -> None in
  let extension =
    match (declared, !specific, generated) with
    | Some ext, _, _ -> Some ext
    | None, Some _, _ | None, None, false -> None
    | None, None, true -> !extension
  in
  Ok { product; pattern; extension; format = !format; keys }

let reserved (format : entity_format) =
  let literal =
    String.concat ""
      (format.separator
      :: List.filter_map
           (function Pattern_text text -> Some text.value | _ -> None)
           format.pattern.segments)
  in
  List.filter
    (fun c -> not ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')))
    (List.of_seq (String.to_seq literal))

let encode ?(reserved = []) source =
  let buffer = Buffer.create (String.length source) in
  String.iter
    (function
      | ('a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '-' | '_' | '.') as c when not (List.mem c reserved)
        ->
          Buffer.add_char buffer c
      | c -> Buffer.add_string buffer (Printf.sprintf "%%%02X" (Char.code c)))
    source;
  Buffer.contents buffer

let decode text =
  let length = String.length text in
  let buffer = Buffer.create length in
  let hex c =
    match c with
    | '0' .. '9' -> Some (Char.code c - 48)
    | 'A' .. 'F' -> Some (Char.code c - 55)
    | 'a' .. 'f' -> Some (Char.code c - 87)
    | _ -> None
  in
  let rec walk i =
    if i < length then
      match
        (text.[i], if i + 2 < length then (hex text.[i + 1], hex text.[i + 2]) else (None, None))
      with
      | '%', (Some high, Some low) ->
          Buffer.add_char buffer (Char.chr ((high * 16) + low));
          walk (i + 3)
      | c, _ ->
          Buffer.add_char buffer c;
          walk (i + 1)
  in
  walk 0;
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
  | None -> error ~span:rule.pattern.span (Problem.Entities_without_format rule.product.name)
  | Some format ->
      let reserved = reserved format in
      let* parts =
        read_list
          (fun (dim, value) ->
            let dim = symbol_text symbols dim in
            let key = Option.value (Hashtbl.find_opt rule.keys dim) ~default:dim in
            let+ text, _ =
              render_parts
                (function
                  | Entity_key -> Ok (Some (encode ~reserved key))
                  | Entity_value -> Ok (Some (encode ~reserved (symbol_text symbols value)))
                  | _ -> error ~span:format.span (Problem.Entity_format_invalid rule.product.name))
                format.pattern.segments
            in
            text)
          (Array.to_list entities)
      in
      Ok (String.concat format.separator parts)

let final_extension rule =
  match rule.extension with Some (Replace ext) -> Some ext | _ -> rule.pattern.suffix

let properties path extension =
  let base = Filename.basename path in
  let stem =
    match extension with
    | Some ext when String.ends_with ~suffix:ext base && base <> ext ->
        String.sub base 0 (String.length base - String.length ext)
    | _ -> Filename.remove_extension base
  in
  (Filename.dirname path, stem)

let unresolved rule name =
  error ~span:rule.pattern.span
    (Problem.Reference_unknown { product = rule.product.name; reference = name })

let normalise path =
  String.concat "/"
    (List.filter (fun part -> part <> "." && part <> "") (String.split_on_char '/' path))

let render ?refer rule symbols entities =
  let refer = Option.value refer ~default:(fun name -> unresolved rule name) in
  let value = function
    | Artifact_property (name, property) ->
        let+ path, extension = refer name in
        let dir, stem = properties path extension in
        Some (match property with Dir -> dir | Stem -> stem)
    | Product_name -> Ok (Some rule.product.path_name)
    | Stage_name ->
        Ok (if rule.product.stage = [] then None else Some (String.concat "/" rule.product.stage))
    | Entity_names ->
        let+ text = entities_format rule symbols entities in
        if text = "" then None else Some text
    | Dimension_name (name, _) ->
        Ok
          (Option.bind (Hashtbl.find_opt symbols.ids name) (fun dim ->
               Option.map
                 (fun (_, value) -> encode (symbol_text symbols value))
                 (Array.find_opt (fun (d, _) -> d = dim) entities)))
    | Entity_key | Entity_value ->
        error ~span:rule.pattern.span (Problem.Path_placeholder_invalid rule.product.name)
  in
  let* path, missing = render_parts value rule.pattern.segments in
  let path = normalise path in
  match missing with
  | Some span -> error ~span (Problem.Path_value_missing rule.product.name)
  | None -> (
      if
        path = ""
        || (not (Filename.is_relative path))
        || List.mem ".." (String.split_on_char '/' path)
      then error ~span:rule.pattern.span (Problem.Path_escapes_root rule.product.name)
      else
        match rule.extension with
        | None -> Ok path
        | Some (Append _) ->
            error (Problem.Not_implemented (rule.product.name, "appended extensions are"))
        | Some (Replace ext) -> (
            match rule.pattern.suffix with
            | None -> Ok (path ^ ext)
            | Some literal when String.ends_with ~suffix:ext literal -> Ok path
            | Some _ ->
                error ~span:rule.pattern.span (Problem.Path_extension_conflict rule.product.name)))

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
    let checked = resolved.checked in
    let refer name =
      match Hashtbl.find_opt checked.names name with
      | None -> unresolved rule name
      | Some id ->
          let referenced = product checked id in
          let* other = prepare referenced (not (Array.mem id checked.sources)) in
          let entities =
            Array.of_list
              (List.filter
                 (fun (dim, _) -> Array.mem dim referenced.dims)
                 (Array.to_list artifact.entities))
          in
          let+ path = render other checked.symbols entities in
          (path, final_extension other)
    in
    let* path = render ~refer rule checked.symbols artifact.entities in
    if Hashtbl.mem used path then
      error (Problem.Path_collision { product = rule.product.name; path })
    else (
      Hashtbl.add used path ();
      Ok path)
  in
  let+ paths = read_list bind_artifact (Array.to_list resolved.artifacts) in
  Array.of_list paths
