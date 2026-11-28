open Ast
open Data

type annotations = {
  mutable filters : (Symbol.t * Symbol.t) list;
  mutable pick : (Symbol.t * bool) option;
  mutable same : Symbol.t array option;
  mutable varying : Symbol.t array;
  mutable each : Symbol.t array;
}

type context = { symbols : symbols; dimensions : Symbol.t array; param : param }

let named context dims span =
  let dimensions = Array.of_list (List.map (intern context.symbols) dims) in
  let absent =
    List.filter (fun d -> not (has_dim context.dimensions (intern context.symbols d))) dims
  in
  let repeated = List.filter (fun d -> List.length (List.filter (( = ) d) dims) > 1) dims in
  if absent <> [] then
    error ~span (Problem.Selector_absent { input = context.param.name; dims = absent })
  else if repeated <> [] then
    error ~span
      (Problem.Selector_repeated
         { input = context.param.name; dims = List.sort_uniq compare repeated })
  else Ok dimensions

let pin context annotations span (name, value) =
  let* dims = named context [ name ] span in
  let dim = dims.(0) in
  if List.mem_assoc dim annotations.filters then
    error ~span (Problem.Where_repeated context.param.name)
  else (
    annotations.filters <- (dim, intern context.symbols value) :: annotations.filters;
    Ok ())

let read_annotation context annotations (annotation : iteration located) =
  match annotation.value with
  | Where pairs ->
      let+ _ = read_list (pin context annotations annotation.span) pairs in
      ()
  | (First _ | Last _) when annotations.pick <> None ->
      error ~span:annotation.span (Problem.Second_pick context.param.name)
  | First name | Last name ->
      let+ dims = named context [ name ] annotation.span in
      annotations.pick <- Some (dims.(0), match annotation.value with Last _ -> true | _ -> false)
  | Same dims ->
      let+ dims = named context dims annotation.span in
      annotations.same <- Some dims
  | Vary dims ->
      let+ dims = named context dims annotation.span in
      annotations.varying <- dims
  | Each dims ->
      let+ dims = named context dims annotation.span in
      annotations.each <- dims

let validate_annotations context annotations span =
  let pinned d = List.mem_assoc d annotations.filters in
  let used =
    Array.to_list annotations.varying @ Array.to_list annotations.each
    @ Option.fold ~none:[] ~some:(fun (d, _) -> [ d ]) annotations.pick
  in
  let* () =
    match List.filter pinned used with
    | [] -> Ok ()
    | clashing ->
        error ~span
          (Problem.Pinned_used
             {
               input = context.param.name;
               dims = List.sort_uniq compare (List.map (symbol_text context.symbols) clashing);
             })
  in
  if
    context.param.many
    && (Array.length annotations.varying = 0
       || Array.length annotations.each <> 0
       || annotations.pick <> None || annotations.same <> None)
  then error ~span (Problem.Many_selectors context.param.name)
  else if (not context.param.many) && Array.length annotations.varying <> 0 then
    error ~span (Problem.Vary_needs_many context.param.name)
  else if Array.length annotations.each <> 0 && annotations.same <> None then
    error ~span (Problem.Each_with_same context.param.name)
  else Ok ()

let describe symbols dimensions (param : param) (arg : arg) =
  let context = { symbols; dimensions; param } in
  let annotations = { filters = []; pick = None; same = None; varying = [||]; each = [||] } in
  let* _ = read_list (read_annotation context annotations) arg.iterations in
  let* () = validate_annotations context annotations arg.span in
  let effective =
    Array.of_list
      (List.filter
         (fun d ->
           (not (List.mem_assoc d annotations.filters))
           && not
                (Option.fold ~none:false ~some:(fun (selected, _) -> d = selected) annotations.pick))
         (Array.to_list dimensions))
  in
  let matching =
    match annotations.same with
    | Some dims -> dims
    | None ->
        Array.of_list
          (List.filter (fun d -> not (has_dim annotations.varying d)) (Array.to_list effective))
  in
  Ok
    ({
       filters = annotations.filters;
       pick = annotations.pick;
       effective;
       matching;
       varying = annotations.varying;
       each = annotations.each;
     }
      : selection)

let dimensions symbols names order selections driver =
  let base = match driver with None -> [||] | Some i -> selections.(i).matching in
  let text = symbol_text symbols in
  let broadcast = Hashtbl.create 8 in
  let* _ =
    read_list
      (fun i ->
        let+ _ =
          read_list
            (fun dim ->
              let collector =
                List.find_opt
                  (fun j -> has_dim selections.(j).varying dim)
                  (List.init (Array.length selections) Fun.id)
              in
              match (driver, Hashtbl.find_opt broadcast dim, collector) with
              | Some d, _, _ when has_dim base dim ->
                  error (Problem.Broadcast_on_driver { driver = names.(d); dim = text dim })
              | _, Some other, _ ->
                  error
                    (Problem.Broadcast_twice
                       { first = names.(other); second = names.(i); dim = text dim })
              | _, None, Some j ->
                  error
                    (Problem.Broadcast_restores
                       { input = names.(i); dim = text dim; collector = names.(j) })
              | _ ->
                  Hashtbl.add broadcast dim i;
                  Ok ())
            (Array.to_list selections.(i).each)
        in
        ())
      (List.init (Array.length selections) Fun.id)
  in
  let output =
    Array.of_list (List.filter (fun d -> has_dim base d || Hashtbl.mem broadcast d) order)
  in
  let* _ =
    read_list
      (fun i ->
        match
          List.filter (fun d -> not (has_dim output d)) (Array.to_list selections.(i).matching)
        with
        | [] -> Ok ()
        | absent ->
            error
              (Problem.Driver_lacks
                 {
                   input = names.(i);
                   driver = Option.map (fun d -> names.(d)) driver;
                   dims = List.map text absent;
                 }))
      (List.init (Array.length selections) Fun.id)
  in
  Ok output

let select symbols (get : Artifact_id.t -> Data.artifact) compare_artifacts (selection : selection)
    ids =
  let passes id =
    let artifact = get id in
    List.for_all
      (fun (dim, value) -> Array.exists (( = ) (dim, value)) artifact.entities)
      selection.filters
  in
  let ids = List.filter passes ids in
  match selection.pick with
  | None -> Ok ids
  | Some (dim, last) ->
      let groups = Hashtbl.create 16 in
      let* _ =
        read_list
          (fun id ->
            let artifact = get id in
            let+ key = project "selection" selection.effective artifact.entities in
            match Hashtbl.find_opt groups key with
            | None -> Hashtbl.add groups key id
            | Some previous ->
                let value (artifact : artifact) =
                  snd (Option.get (Array.find_opt (fun (d, _) -> d = dim) artifact.entities))
                in
                let compared =
                  natural
                    (symbol_text symbols (value artifact))
                    (symbol_text symbols (value (get previous)))
                in
                if (last && compared > 0) || ((not last) && compared < 0) then
                  Hashtbl.replace groups key id)
          ids
      in
      Ok (Hashtbl.fold (fun _ id acc -> id :: acc) groups [] |> List.sort compare_artifacts)
