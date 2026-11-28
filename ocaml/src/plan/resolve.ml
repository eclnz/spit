open Ast
open Data

type state = {
  checked : checked;
  artifacts : artifact Table.t;
  jobs : job Table.t;
  families : Artifact_id.t list array;
}

type inputs = {
  step : step;
  params : param array;
  selected : Artifact_id.t list array;
  indexes : (Symbol.t list, Artifact_id.t list) Hashtbl.t array;
}

let artifact state id = Table.get state.artifacts (Artifact_id.index id)

let add_artifact state product entities producer =
  let id = Artifact_id.Artifact_id (Table.add state.artifacts { product; entities; producer }) in
  let index = Product_id.index product in
  state.families.(index) <- id :: state.families.(index);
  id

let compare_artifacts state a b =
  let values id = Array.to_list (artifact state id).entities |> List.map snd in
  compare_values state.checked.symbols (values a) (values b)

let sort_family state id =
  let index = Product_id.index id in
  state.families.(index) <- List.sort (compare_artifacts state) state.families.(index)

let admit_source state source_set identities (record : Inventory.record) =
  match Hashtbl.find_opt state.checked.names record.product with
  | None -> error record.product "inventory names an unknown source"
  | Some id when not (Hashtbl.mem source_set id) ->
      error record.product "inventory may contain only source artifacts"
  | Some id ->
      let p = product state.checked id in
      let values =
        List.map
          (fun (k, v) -> (intern state.checked.symbols k, intern state.checked.symbols v))
          record.values
      in
      if
        List.length values <> Array.length p.dims
        || List.length (List.sort_uniq compare (List.map fst values)) <> List.length values
        || not (List.for_all (fun (d, _) -> has_dim p.dims d) values)
      then error p.name "inventory dimensions differ from the source declaration"
      else
        let entities = Array.map (fun dim -> (dim, List.assoc dim values)) p.dims in
        if Hashtbl.mem identities (id, entities) then error p.name "duplicate inventory identity"
        else (
          Hashtbl.add identities (id, entities) ();
          ignore (add_artifact state id entities None);
          Ok ())

let admit_inventory state inventory =
  let sources = Hashtbl.create (Array.length state.checked.sources)
  and identities = Hashtbl.create 64 in
  Array.iter (fun id -> Hashtbl.add sources id ()) state.checked.sources;
  let+ _ = read_list (admit_source state sources identities) inventory.Inventory.records in
  Array.iter (sort_family state) state.checked.sources

let index_input state (param : param) selection ids =
  let table = Hashtbl.create 32 in
  let+ _ =
    read_list
      (fun id ->
        let+ key = project param.name selection.matching (artifact state id).entities in
        let previous = Option.value (Hashtbl.find_opt table key) ~default:[] in
        Hashtbl.replace table key (id :: previous))
      ids
  in
  Hashtbl.filter_map_inplace (fun _ ids -> Some (List.rev ids)) table;
  table

let prepare_inputs state (step : step) =
  let params = Array.of_list step.operation.params in
  let ports = List.init (Array.length step.inputs) Fun.id in
  let* selected =
    read_list
      (fun port ->
        Selection.select state.checked.symbols (artifact state) (compare_artifacts state)
          step.selections.(port)
          state.families.(Product_id.index step.inputs.(port)))
      ports
  in
  let selected = Array.of_list selected in
  let+ indexes =
    read_list
      (fun port -> index_input state params.(port) step.selections.(port) selected.(port))
      ports
  in
  { step; params; selected; indexes = Array.of_list indexes }

let broadcast_table state (param : param) selection common ids =
  let table = Hashtbl.create 32 in
  let+ _ =
    read_list
      (fun id ->
        let entities = (artifact state id).entities in
        let* key = project param.name common entities in
        let+ values = project param.name selection.effective entities in
        let binding = Array.of_list (List.combine (Array.to_list selection.effective) values) in
        let previous = Option.value (Hashtbl.find_opt table key) ~default:[] in
        Hashtbl.replace table key (binding :: previous))
      ids
  in
  table

let broadcast state inputs (contexts, current_dims) port =
  let selection = inputs.step.selections.(port) in
  if Array.length selection.each = 0 then Ok (contexts, current_dims)
  else
    let common =
      Array.of_list (List.filter (has_dim current_dims) (Array.to_list selection.effective))
    in
    let* table =
      broadcast_table state inputs.params.(port) selection common inputs.selected.(port)
    in
    let+ extended =
      read_list
        (fun context ->
          let+ key = project "broadcast" common context in
          let matches = Option.value (Hashtbl.find_opt table key) ~default:[] in
          List.map
            (fun binding ->
              Array.append context
                (Array.of_list
                   (List.filter
                      (fun (d, _) -> not (has_dim current_dims d))
                      (Array.to_list binding))))
            matches)
        contexts
    in
    ( List.concat extended,
      Array.of_list
        (List.sort_uniq compare (Array.to_list current_dims @ Array.to_list selection.effective)) )

let contexts state inputs =
  let base =
    match inputs.step.driver with
    | None -> [||]
    | Some port -> inputs.step.selections.(port).matching
  in
  let initial =
    match inputs.step.driver with
    | None -> [ [||] ]
    | Some driver ->
        Hashtbl.fold
          (fun values _ acc -> Array.of_list (List.combine (Array.to_list base) values) :: acc)
          inputs.indexes.(driver) []
  in
  let+ contexts, _ =
    List.fold_left
      (fun result port ->
        let* state_ = result in
        broadcast state inputs state_ port)
      (Ok (initial, base))
      (List.init (Array.length inputs.params) Fun.id)
  in
  contexts

let render_values state dims values =
  String.concat ","
    (List.map2
       (fun d v -> symbol_text state.checked.symbols d ^ "=" ^ symbol_text state.checked.symbols v)
       dims values)

let matching_inputs state inputs entities =
  let+ values =
    read_list
      (fun port ->
        let dims = inputs.step.selections.(port).matching in
        let* key = project inputs.params.(port).name dims entities in
        let at = render_values state (Array.to_list dims) key in
        let at = if at = "" then "" else " at " ^ at in
        match Hashtbl.find_opt inputs.indexes.(port) key with
        | None ->
            error inputs.params.(port).name
              ("missing input for `" ^ inputs.step.operation.name ^ "`" ^ at)
        | Some ids when (not inputs.params.(port).many) && List.length ids <> 1 ->
            error inputs.params.(port).name
              (Printf.sprintf "ambiguous input: %d artifacts match%s" (List.length ids) at)
        | Some ids when inputs.params.(port).many && List.length ids < inputs.step.minimum ->
            error inputs.params.(port).name
              (Printf.sprintf "collection%s has %d artifacts, fewer than @min(%d) requires" at
                 (List.length ids) inputs.step.minimum)
        | Some ids -> Ok (Array.of_list ids))
      (List.init (Array.length inputs.params) Fun.id)
  in
  Array.of_list values

let align_members state inputs values =
  let many =
    List.filter (fun i -> inputs.params.(i).many) (List.init (Array.length inputs.params) Fun.id)
  in
  let members port =
    read_list
      (fun id -> project "collection member" inputs.step.varying (artifact state id).entities)
      (Array.to_list values.(port))
  in
  match many with
  | [] -> Ok ()
  | first :: rest ->
      let* expected = members first in
      let varying = Array.to_list inputs.step.varying in
      let+ _ =
        read_list
          (fun port ->
            let* actual = members port in
            let only_in name members other_name others =
              List.find_opt (fun member -> not (List.mem member others)) members
              |> Option.map (fun member ->
                  Printf.sprintf "`%s` has %s and `%s` does not" name
                    (render_values state varying member)
                    other_name)
            in
            let port_name = inputs.params.(port).name and first_name = inputs.params.(first).name in
            if actual = expected then Ok ()
            else
              let detail =
                match only_in port_name actual first_name expected with
                | Some detail -> detail
                | None ->
                    Option.value
                      (only_in first_name expected port_name actual)
                      ~default:"their members are in different orders"
              in
              error inputs.step.operation.name ("parallel collections differ: " ^ detail))
          rest
      in
      ()

let make_job state index inputs dims values =
  let entities = Array.of_list (List.combine (Array.to_list dims) values) in
  let* matched = matching_inputs state inputs entities in
  let* () = align_members state inputs matched in
  let depends_on =
    Array.to_list matched |> List.concat_map Array.to_list
    |> List.filter_map (fun id -> (artifact state id).producer)
    |> List.sort_uniq compare
  in
  let id = Job_id.Job_id (state.jobs.length + 1) in
  let outputs =
    Array.map (fun product -> add_artifact state product entities (Some id)) inputs.step.outputs
  in
  ignore
    (Table.add state.jobs { id; step = index; inputs = matched; outputs; entities; depends_on });
  Ok ()

let resolve_step state index (step : step) =
  let dims = (product state.checked step.outputs.(0)).dims in
  let* inputs = prepare_inputs state step in
  let* contexts = contexts state inputs in
  let* keys = read_list (project "job" dims) contexts in
  let keys = List.sort_uniq (compare_values state.checked.symbols) keys in
  let+ _ = read_list (make_job state index inputs dims) keys in
  Array.iter (sort_family state) step.outputs

let resolve checked (inventory : Inventory.t) =
  let state =
    {
      checked;
      artifacts = Table.create ();
      jobs = Table.create ();
      families = Array.make (Array.length checked.products) [];
    }
  in
  let* () = admit_inventory state inventory in
  let+ _ =
    read_list
      (fun i -> resolve_step state i checked.steps.(i))
      (List.init (Array.length checked.steps) Fun.id)
  in
  { checked; artifacts = Table.to_array state.artifacts; jobs = Table.to_array state.jobs }
