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
            error ~span (Problem.Two_shapes { product = name; dim; first = other; second = shape })
        | _ ->
            Hashtbl.replace shapes dim shape;
            walk rest)
    | _ :: rest -> walk rest
  in
  walk parts

let open_length = function None -> true | Some shape -> Shape.fixed_length shape = None

let separated name dropped parts =
  let rec walk = function
    | Pattern_value { value = Dimension_name (a, shape_a); span }
      :: (Pattern_value { value = Dimension_name (b, shape_b); _ } :: _ as rest) ->
        if open_length shape_a && open_length shape_b then
          if shape_a <> None && shape_b <> None then
            error ~span (Problem.Touching_shapes { product = name; dropped })
          else
            error ~span (Problem.Touching_values { product = name; dropped; first = a; second = b })
        else walk rest
    | _ :: rest -> walk rest
    | [] -> Ok ()
  in
  walk parts

let extension checked (product : product) (prepared : Paths.rule) =
  let declared = match product.shape with File ext -> ext | _ -> None in
  match (prepared.extension, prepared.pattern.suffix) with
  | Some (Replace ext), Some literal when not (String.ends_with ~suffix:ext literal) -> (
      let span = prepared.pattern.span and name = product.name in
      let writer =
        match (declared, producer checked product) with
        | None, _ -> None
        | Some _, Some operation -> Some (Problem.Operation_writes operation)
        | Some _, None -> Some (Problem.Source_declares name)
      in
      match (specific product, writer) with
      | true, Some writer ->
          error ~span (Problem.Rule_extension { product = name; literal; writer; ext })
      | false, Some writer when is_source checked product ->
          error ~span (Problem.Default_extension_source { literal; writer; ext })
      | false, Some writer ->
          error ~span (Problem.Default_extension_output { literal; writer; ext })
      | _, None -> error ~span (Problem.Default_extension_setting { product = name; literal; ext }))
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
      error ~span (Problem.Rule_unknown_dimension { product = product.name; dim = name })
  | _, name :: _ ->
      let span = Option.value (placeholder name prepared.pattern.segments) ~default:span in
      error ~span
        (Problem.Absent_dimension
           { product = product.name; dim = name; own_rule = specific product })
  | _ -> (
      let entities = uses Entity_names parts in
      if entities && dims = [] then error ~span (Problem.Entities_without_dimensions product.name)
      else
        match List.filter (fun dim -> not (List.mem dim (names parts))) dims with
        | dim :: _ when (not entities) && references parts = [] ->
            error ~span (Problem.Omitted_dimension { product = product.name; dim })
        | _ -> Ok ())

let settings checked (product : product) =
  read_list
    (fun (setting : Setting.t located) ->
      match setting.value with
      | Path (Every_artifact, pattern) when shaped pattern.segments ->
          error ~span:pattern.span Problem.Shape_in_default
      | Path (Named_artifact name, pattern)
        when name = product.name && (not (is_source checked product)) && shaped pattern.segments ->
          error ~span:pattern.span (Problem.Shape_on_output name)
      | _ -> Ok ())
    product.settings

let reference checked (product : product) span name =
  match Hashtbl.find_opt checked.names name with
  | None -> error ~span (Problem.Reference_unknown { product = product.name; reference = name })
  | Some id when id = product.id -> error ~span (Problem.Reference_self product.name)
  | Some id -> (
      let other = Data.product checked id in
      if is_source checked product && not (is_source checked other) then
        error ~span (Problem.Reference_not_source { product = product.name; reference = name })
      else if
        List.sort compare (Array.to_list other.dims)
        <> List.sort compare (Array.to_list product.dims)
      then error ~span (Problem.Reference_dimensions { product = product.name; reference = name })
      else
        match
          Option.map
            (fun (p : pattern) -> references p.segments)
            (rule_pattern other.name other.settings)
        with
        | Some (next :: _) ->
            error ~span (Problem.Reference_chain { product = product.name; reference = name; next })
        | _ -> Ok ())

let product_rule checked known (product : product) =
  let* _ = settings checked product in
  let* _ =
    match rule_pattern product.name product.settings with
    | None -> Ok []
    | Some pattern ->
        read_list (reference checked product pattern.span) (references pattern.segments)
  in
  match rule checked product with
  | None when is_source checked product -> Ok ()
  | None -> error (Problem.Output_no_path_rule product.name)
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
               | Pattern_value { value = Artifact_property (name, Dir); _ } -> "{" ^ name ^ "$dir}"
               | Pattern_value { value = Artifact_property (name, Stem); _ } ->
                   "{" ^ name ^ "$stem}"
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
                error (Problem.Same_path { first = other; second = product.name; path = fst key })
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
        if formats = 0 then error ~span (Problem.Label_without_format dim)
        else if not (List.mem dim known) then error ~span (Problem.Label_unknown dim)
        else if label <> dim && List.mem label known then
          error ~span (Problem.Label_is_dimension { dim; label })
        else if List.length (List.filter (fun (d, _, _) -> d = dim) renames) > 1 then
          error ~span (Problem.Label_twice dim)
        else Ok ())
      (List.rev renames)
  in
  ()

let validate checked =
  let known =
    List.sort_uniq compare
      (List.concat_map (dimension_names checked.symbols) (Array.to_list checked.products))
  in
  let problems = ref [] in
  let products = Array.to_list checked.products in
  let _ = each problems (labels known) products in
  let _ = each problems (product_rule checked known) products in
  if !problems = [] then ignore (keep problems (distinct checked));
  match !problems with [] -> Ok () | problems -> Error (List.rev problems)
