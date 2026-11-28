open Ast
open Data

let memory text =
  let length = String.length text in
  if length < 2 then error Problem.Memory_format
  else
    let* factor =
      match text.[length - 1] with
      | 'K' -> Ok 1024
      | 'M' -> Ok 1048576
      | 'G' -> Ok 1073741824
      | 'T' -> Ok 1099511627776
      | _ -> error Problem.Memory_unit
    in
    let number = String.sub text 0 (length - 1) in
    match int_of_string_opt number with
    | Some n
      when n >= 0
           && String.for_all (fun c -> c >= '0' && c <= '9') number
           && n <= 9007199254740992 / factor ->
        Ok (n * factor)
    | _ -> error Problem.Memory_range

let add_named table name value span =
  if Hashtbl.mem table name then error ~span (Problem.Setting_repeated name)
  else (
    Hashtbl.add table name value;
    Ok ())

let named_settings settings =
  let resources = Hashtbl.create 16 and anchored = Hashtbl.create 16 in
  let+ _ =
    read_list
      (fun (setting : Setting.t located) ->
        match setting.value with
        | Resources (Named_operation name, values) -> add_named resources name values setting.span
        | Anchored (Named_operation name) -> add_named anchored name true setting.span
        | _ -> Ok ())
      settings
  in
  (resources, anchored)

let apply_resource configuration (key, value) =
  match key with
  | "cores" -> (
      match int_of_string_opt value with
      | Some cores when cores >= 1 -> Ok { configuration with cores }
      | _ -> error Problem.Cores_value)
  | "memory" ->
      let+ memory = memory value in
      { configuration with memory = Some memory }
  | _ -> error (Problem.Resource_unknown key)

let apply_resources configuration values =
  let seen = Hashtbl.create 2 in
  List.fold_left
    (fun result (key, value) ->
      let* configuration = result in
      if Hashtbl.mem seen key then error (Problem.Resource_repeated key)
      else (
        Hashtbl.add seen key ();
        apply_resource configuration (key, value)))
    (Ok configuration) values

let operation_configuration owners resources anchored (operation : operation_decl) =
  let owner = Option.value (Hashtbl.find_opt owners operation.name) ~default:[] in
  let lists =
    List.filter_map
      (fun (s : Setting.t located) ->
        match s.value with Resources (Every_operation, v) -> Some v | _ -> None)
      owner
  in
  let lists =
    lists @ match Hashtbl.find_opt resources operation.name with None -> [] | Some v -> [ v ]
  in
  let anchored =
    Hashtbl.mem anchored operation.name
    || List.exists (fun (s : Setting.t located) -> s.value = Setting.Anchored Every_operation) owner
  in
  List.fold_left
    (fun result values ->
      let* configuration = result in
      apply_resources configuration values)
    (Ok { cores = 1; memory = None; anchored })
    lists

let configurations owners settings operations =
  let* resources, anchored = named_settings settings in
  let result = Hashtbl.create 32 in
  let+ _ =
    read_list
      (fun (operation : operation_decl) ->
        let+ configuration = operation_configuration owners resources anchored operation in
        Hashtbl.add result operation.name configuration)
      operations
  in
  result
