open Ast
open Data

let rec names parts =
  List.concat_map
    (function
      | Pattern_value { value = Dimension_name (name, _); _ } -> [ name ]
      | Pattern_optional parts -> names parts
      | _ -> [])
    parts

let rec placeholder name parts =
  List.find_map
    (function
      | Pattern_value { value = Dimension_name (found, _); span } when found = name -> Some span
      | Pattern_optional parts -> placeholder name parts
      | _ -> None)
    parts

let rec uses value parts =
  List.exists
    (function
      | Pattern_value part -> part.value = value
      | Pattern_optional parts -> uses value parts
      | _ -> false)
    parts

let rec shaped parts =
  List.exists
    (function
      | Pattern_value { value = Dimension_name (_, Some _); _ } -> true
      | Pattern_optional parts -> shaped parts
      | _ -> false)
    parts

let dimension_names symbols (product : product) =
  List.map (symbol_text symbols) (Array.to_list product.dims)

let flatten symbols (product : product) parts =
  let dims = dimension_names symbols product in
  let dropped = ref false in
  let kept inner =
    List.for_all (fun name -> List.mem name dims) (names inner)
    && ((not (uses Entity_names inner)) || dims <> [])
    && ((not (uses Stage_name inner)) || product.stage <> [])
  in
  let rec walk parts =
    List.concat_map
      (function
        | Pattern_optional inner ->
            if kept inner then walk inner
            else (
              dropped := true;
              [])
        | part -> [ part ])
      parts
  in
  let flat = walk parts in
  (flat, !dropped)

let is_source checked (product : product) = Array.mem product.id checked.sources

let specific (product : product) =
  List.exists
    (fun (setting : Setting.t located) ->
      match setting.value with Path (Named_artifact name, _) -> name = product.name | _ -> false)
    product.settings

let producer checked (product : product) =
  Array.find_opt (fun (step : step) -> Array.mem product.id step.outputs) checked.steps
  |> Option.map (fun (step : step) -> step.operation.name)

let absent checked (product : product) (prepared : Paths.rule) =
  let parts, _ = flatten checked.symbols product prepared.pattern.segments in
  let dims = dimension_names checked.symbols product in
  List.filter (fun name -> not (List.mem name dims)) (names parts)

let rule checked (product : product) =
  match Paths.prepare product (not (is_source checked product)) with
  | Error _ -> None
  | Ok prepared ->
      let parts, _ = flatten checked.symbols product prepared.pattern.segments in
      let unusable =
        absent checked product prepared <> [] || (uses Entity_names parts && product.dims = [||])
      in
      if is_source checked product && (not (specific product)) && unusable then None
      else Some prepared

let one_shape name parts =
  let shapes = Hashtbl.create 4 in
  let rec walk = function
    | [] -> Ok ()
    | Pattern_value { value = Dimension_name (dim, Some shape); span } :: rest -> (
        match Hashtbl.find_opt shapes dim with
        | Some other when other <> shape ->
            error ~span name
              (Printf.sprintf "`%s` has two shapes, `%s` and `%s`" dim (Shape.name other)
                 (Shape.name shape))
        | _ ->
            Hashtbl.replace shapes dim shape;
            walk rest)
    | _ :: rest -> walk rest
  in
  walk parts

let open_length = function None -> true | Some shape -> Shape.fixed_length shape = None

let separated name dropped parts =
  let context =
    if dropped then
      Printf.sprintf "in the path rule for `%s`, once the groups its dimensions lack are dropped, "
        name
    else ""
  in
  let rec walk = function
    | Pattern_value { value = Dimension_name (a, shape_a); span }
      :: (Pattern_value { value = Dimension_name (b, shape_b); _ } :: _ as rest) ->
        if open_length shape_a && open_length shape_b then
          if shape_a <> None && shape_b <> None then
            error ~span name (context ^ "no text between two shapes of any length")
          else
            error ~span name
              (Printf.sprintf
                 "%s`{%s}{%s}` has no text between two values of any length, so a file's name does \
                  not say where one ends; put a `-` or `_` between them"
                 context a b)
        else walk rest
    | _ :: rest -> walk rest
    | [] -> Ok ()
  in
  walk parts

let extension checked (product : product) (prepared : Paths.rule) =
  let declared = match product.shape with File ext -> ext | _ -> None in
  match (prepared.extension, prepared.pattern.suffix) with
  | Some (Replace ext), Some literal when not (String.ends_with ~suffix:ext literal) -> (
      let span = prepared.pattern.span in
      let writer =
        match (declared, producer checked product) with
        | None, _ -> None
        | Some _, Some operation -> Some (Printf.sprintf "operation `%s` writes `%s`" operation ext)
        | Some _, None -> Some (Printf.sprintf "source `%s` declares `%s`" product.name ext)
      in
      match (specific product, writer) with
      | true, Some writer ->
          error ~span product.name
            (Printf.sprintf
               "path `%s` ends in `%s`, but %s; drop the extension or use `%s`; SPIT reads the \
                extension from the first `.` after the last placeholder, so keep `.` out of the \
                name before it"
               product.name literal writer ext)
      | false, Some writer when is_source checked product ->
          error ~span product.name
            (Printf.sprintf
               "the default path ends in `%s`, but %s; write the default path without an \
                extension, so each source's declared extension completes it"
               literal writer)
      | false, Some writer ->
          error ~span product.name
            (Printf.sprintf
               "the default path ends in `%s`, but %s; write the default path without an \
                extension, and give the outputs that use it `ext: %s`"
               literal writer literal)
      | _, None ->
          error ~span product.name
            (Printf.sprintf
               "the default path for `%s` ends in `%s`, but `default ext:` sets `%s`; write the \
                extension once, with `default ext:`"
               product.name literal ext))
  | _ -> Ok ()

let coverage checked known (product : product) (prepared : Paths.rule) =
  let span = prepared.pattern.span in
  let parts, _ = flatten checked.symbols product prepared.pattern.segments in
  let dims = dimension_names checked.symbols product in
  let unknown =
    List.filter (fun name -> not (List.mem name known)) (names prepared.pattern.segments)
  in
  match (unknown, absent checked product prepared) with
  | name :: _, _ when not (specific product) ->
      let span = Option.value (placeholder name prepared.pattern.segments) ~default:span in
      error ~span product.name (Printf.sprintf "path rule names `{%s}`, which no product has" name)
  | _, name :: _ ->
      let span = Option.value (placeholder name prepared.pattern.segments) ~default:span in
      let own =
        if specific product then ""
        else
          Printf.sprintf ", or give `%s` a rule of its own with `path %s:`" product.name
            product.name
      in
      error ~span product.name
        (Printf.sprintf
           "path template for `%s` uses absent dimension `%s`; put it in `[...]` if only some \
            products have it%s"
           product.name name own)
  | _ -> (
      let entities = uses Entity_names parts in
      if entities && dims = [] then
        error ~span product.name
          (Printf.sprintf
             "path rule for `%s` uses `{@entities}`, but `%s` has no dimensions; put it in \
              `[...]`, as `[{@entities}_]`"
             product.name product.name)
      else
        match List.filter (fun dim -> not (List.mem dim (names parts))) dims with
        | dim :: _ when not entities ->
            error ~span product.name
              (Printf.sprintf
                 "path rule for `%s` omits dimension `%s`, so two of its artifacts could share a \
                  path; add `{%s}` or `{@entities}`"
                 product.name dim dim)
        | _ -> Ok ())

let settings checked (product : product) =
  read_list
    (fun (setting : Setting.t located) ->
      match setting.value with
      | Path (Every_artifact, pattern) when shaped pattern.segments ->
          error ~span:pattern.span "default path"
            "`default path:` is the rule for outputs too; write the shape in a `path` rule for \
             each source, as `path <source>: ...`"
      | Path (Named_artifact name, pattern)
        when name = product.name && (not (is_source checked product)) && shaped pattern.segments ->
          error ~span:pattern.span name
            (Printf.sprintf "shapes narrow a source's path rule only; `%s` is made by a step" name)
      | _ -> Ok ())
    product.settings

let product_rule checked known (product : product) =
  let* _ = settings checked product in
  match rule checked product with
  | None when is_source checked product -> Ok ()
  | None ->
      error product.name
        (Printf.sprintf
           "output `%s` has no path rule; write `path %s: ...`, or a `default path:` for the \
            pipeline or its stage"
           product.name product.name)
  | Some prepared ->
      let* () = coverage checked known product prepared in
      let* () = extension checked product prepared in
      if is_source checked product then
        let parts, dropped = flatten checked.symbols product prepared.pattern.segments in
        let* () = one_shape product.name parts in
        separated product.name dropped parts
      else Ok ()

let template checked (product : product) =
  Option.map
    (fun (prepared : Paths.rule) ->
      let parts, _ = flatten checked.symbols product prepared.pattern.segments in
      let rendered =
        String.concat ""
          (List.map
             (function
               | Pattern_text text -> text.value
               | Pattern_value { value = Dimension_name (name, _); _ } -> "{" ^ name ^ "}"
               | Pattern_value { value = Product_name; _ } -> product.path_name
               | Pattern_value { value = Stage_name; _ } -> String.concat "/" product.stage
               | Pattern_value { value = Entity_names; _ } -> "{@entities}"
               | Pattern_value { value = Entity_key | Entity_value; _ } | Pattern_optional _ -> "")
             parts)
      in
      let extension =
        match (prepared.extension, prepared.pattern.suffix) with
        | Some (Replace ext), None -> ext
        | _ -> ""
      in
      (rendered ^ extension, List.sort compare (dimension_names checked.symbols product)))
    (rule checked product)

let distinct checked =
  let seen = Hashtbl.create 16 in
  let+ _ =
    read_list
      (fun (product : product) ->
        match template checked product with
        | None -> Ok ()
        | Some key -> (
            match Hashtbl.find_opt seen key with
            | Some other ->
                error product.name
                  (Printf.sprintf
                     "products `%s` and `%s` bind to the same path `%s` for the same entities" other
                     product.name (fst key))
            | None ->
                Hashtbl.add seen key product.name;
                Ok ()))
      (Array.to_list checked.products)
  in
  ()

let labels known (product : product) =
  let formats, renames =
    List.fold_left
      (fun (formats, renames) (setting : Setting.t located) ->
        match setting.value with
        | Entities (Every_dimension, Format _) -> (formats + 1, renames)
        | Entities (Named_dimension dim, Rename label) ->
            (formats, (dim, label, setting.span) :: renames)
        | _ -> (formats, renames))
      (0, []) product.settings
  in
  let+ _ =
    read_list
      (fun (dim, label, span) ->
        if formats = 0 then
          error ~span dim
            "an entity label renames a dimension in a `default entities:` spelling, and there is \
             none"
        else if not (List.mem dim known) then
          error ~span dim (Printf.sprintf "an entity label names `%s`, which no product has" dim)
        else if label <> dim && List.mem label known then
          error ~span dim
            (Printf.sprintf
               "the label `%s` is also a dimension, so a file's name would spell more than one \
                dimension the same way"
               label)
        else if List.length (List.filter (fun (d, _, _) -> d = dim) renames) > 1 then
          error ~span dim (Printf.sprintf "`%s` has a duplicate entity label" dim)
        else Ok ())
      (List.rev renames)
  in
  ()

let validate checked =
  let known =
    List.sort_uniq compare
      (List.concat_map (dimension_names checked.symbols) (Array.to_list checked.products))
  in
  let* _ = read_list (labels known) (Array.to_list checked.products) in
  let* _ = read_list (product_rule checked known) (Array.to_list checked.products) in
  distinct checked
