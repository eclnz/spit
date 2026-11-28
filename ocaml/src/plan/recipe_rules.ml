open Data

type comparison = Lt | Le | Eq | Ne | Ge | Gt

type condition =
  | Count of { source : string; comparison : comparison; value : int }
  | Has of { source : string; dim : string; value : string }
  | Missing of { source : string; dim : string; value : string }
  | Contexts of { dims : string list; comparison : comparison; value : int }

type origin = { text : string; line : int; reason : string option }

type exclusion =
  | Artifact of { product : string; values : (string * string) list; origin : origin }
  | Group of { values : (string * string) list; origin : origin }
  | Where of { dims : string list; condition : condition; origin : origin }

type preference = { glob : string; origin : origin }

type requirement = { dims : string list; condition : condition; origin : origin }

type rules = {
  exclusions : exclusion list;
  preferences : preference list;
  requirements : requirement list;
}

let empty = { exclusions = []; preferences = []; requirements = [] }

let split_reason text =
  let rec find i =
    if i >= String.length text then None
    else if text.[i] = '#' && (i = 0 || text.[i - 1] = ' ' || text.[i - 1] = '\t') then Some i
    else find (i + 1)
  in
  match find 0 with
  | None -> (String.trim text, None)
  | Some i ->
      let reason = String.trim (String.sub text (i + 1) (String.length text - i - 1)) in
      (String.trim (String.sub text 0 i), if reason = "" then None else Some reason)

let invalid origin = error (Problem.Recipe_rule origin.text)

let bracketed origin text =
  let text = String.trim text in
  let length = String.length text in
  if length >= 2 && text.[0] = '[' && text.[length - 1] = ']' then
    Ok
      (String.sub text 1 (length - 2)
      |> String.split_on_char ',' |> List.map String.trim
      |> List.filter (( <> ) ""))
  else invalid origin

let pairs origin items =
  read_list
    (fun item ->
      match String.split_on_char '=' item with
      | [ key; value ] when String.trim key <> "" && String.trim value <> "" ->
          Ok (String.trim key, String.trim value)
      | _ -> invalid origin)
    items

let comparison text =
  List.find_map
    (fun (symbol, comparison) ->
      if String.starts_with ~prefix:symbol text then
        Some
          ( comparison,
            String.sub text (String.length symbol) (String.length text - String.length symbol) )
      else None)
    [ ("<=", Le); (">=", Ge); ("!=", Ne); ("<", Lt); (">", Gt); ("=", Eq) ]

let count origin text =
  match comparison text with
  | Some (comparison, number) -> (
      match int_of_string_opt (String.trim number) with
      | Some value -> Ok (comparison, value)
      | None -> invalid origin)
  | None -> invalid origin

let condition origin text =
  match String.split_on_char ' ' (String.trim text) |> List.filter (( <> ) "") with
  | [ subject; measure ] when String.starts_with ~prefix:"count" measure ->
      let* comparison, value = count origin (String.sub measure 5 (String.length measure - 5)) in
      if String.starts_with ~prefix:"contexts[" subject then
        let+ dims = bracketed origin (String.sub subject 8 (String.length subject - 8)) in
        Contexts { dims; comparison; value }
      else Ok (Count { source = subject; comparison; value })
  | [ source; (("has" | "missing") as kind); pair ] -> (
      let* found = pairs origin [ pair ] in
      match (kind, found) with
      | "has", [ (dim, value) ] -> Ok (Has { source; dim; value })
      | _, [ (dim, value) ] -> Ok (Missing { source; dim; value })
      | _ -> invalid origin)
  | _ -> invalid origin

let grouped origin text =
  match String.index_opt text ']' with
  | None -> invalid origin
  | Some close ->
      let head = String.sub text 0 (close + 1) in
      let rest = String.trim (String.sub text (close + 1) (String.length text - close - 1)) in
      let* dims = bracketed origin head in
      if String.starts_with ~prefix:"where " rest then
        let+ condition = condition origin (String.sub rest 6 (String.length rest - 6)) in
        (dims, condition)
      else invalid origin

let origin keyword line text =
  let body, reason = split_reason text in
  ({ text = keyword ^ " " ^ body; line; reason }, body)

let exclusion line text =
  let origin, body = origin "exclude" line text in
  if
    String.starts_with ~prefix:"[" body
    && String.contains body '='
    && not (String.contains body ' ')
  then
    let* items = bracketed origin body in
    let+ values = pairs origin items in
    Group { values; origin }
  else if String.starts_with ~prefix:"[" body then
    let+ dims, condition = grouped origin body in
    Where { dims; condition; origin }
  else
    match String.index_opt body '[' with
    | Some open_at when open_at > 0 ->
        let product = String.sub body 0 open_at in
        let* items = bracketed origin (String.sub body open_at (String.length body - open_at)) in
        let+ values = pairs origin items in
        Artifact { product; values; origin }
    | _ -> invalid origin

let requirement line text =
  let origin, body = origin "require" line text in
  let+ dims, condition = grouped origin body in
  { dims; condition; origin }

let preference line text =
  let origin, body = origin "prefer" line text in
  if String.starts_with ~prefix:"path " body then
    Ok { glob = String.trim (String.sub body 5 (String.length body - 5)); origin }
  else invalid origin

let rec glob pattern path =
  match (pattern, path) with
  | [], [] -> true
  | p :: ps, x :: xs -> component p x && glob ps xs
  | _ -> false

and component pattern text =
  let lp = String.length pattern and lt = String.length text in
  let rec go i j =
    if i = lp then j = lt
    else if pattern.[i] = '*' then go (i + 1) j || (j < lt && go i (j + 1))
    else j < lt && pattern.[i] = text.[j] && go (i + 1) (j + 1)
  in
  go 0 0

let matches_glob pattern path =
  glob (String.split_on_char '/' pattern) (String.split_on_char '/' path)

let has_all pairs values = List.for_all (fun (k, v) -> List.assoc_opt k values = Some v) pairs

let same_values a b = List.sort compare a = List.sort compare b

let compare_with comparison found value =
  match comparison with
  | Lt -> found < value
  | Le -> found <= value
  | Eq -> found = value
  | Ne -> found <> value
  | Ge -> found >= value
  | Gt -> found > value

let project dims values =
  if List.for_all (fun dim -> List.mem_assoc dim values) dims then
    Some (List.map (fun dim -> (dim, List.assoc dim values)) dims)
  else None

let natural_values a b =
  let rec go = function
    | (_, x) :: xs, (_, y) :: ys ->
        let c = natural x y in
        if c <> 0 then c else go (xs, ys)
    | [], [] -> 0
    | [], _ -> -1
    | _, [] -> 1
  in
  go (a, b)

let domain dims (records : Inventory.record list) contexts =
  List.filter_map (fun (r : Inventory.record) -> project dims r.values) records
  @ List.filter_map (project dims) contexts
  |> List.sort_uniq compare |> List.sort natural_values

let evaluate condition group (records : Inventory.record list) contexts =
  let within (r : Inventory.record) source = r.product = source && has_all group r.values in
  match condition with
  | Count { source; comparison; value } ->
      let found = List.length (List.filter (fun r -> within r source) records) in
      (compare_with comparison found value, Some found)
  | Has { source; dim; value } ->
      ( List.exists (fun r -> within r source && List.assoc_opt dim r.values = Some value) records,
        None )
  | Missing { source; dim; value } ->
      ( not
          (List.exists
             (fun r -> within r source && List.assoc_opt dim r.values = Some value)
             records),
        None )
  | Contexts { dims; comparison; value } ->
      let found =
        List.filter (has_all group) contexts
        |> List.filter_map (project dims)
        |> List.sort_uniq compare |> List.length
      in
      (compare_with comparison found value, Some found)

let removal product values origin found =
  Json.Object
    [
      ("product", Json.option Json.str product);
      ( "entities",
        Json.Object (List.map (fun (k, v) -> (k, Json.Text v)) (List.sort compare values)) );
      ("rule", Json.Text origin.text);
      ("origin", Json.Text (Printf.sprintf "line %d" origin.line));
      ("reason", Json.option Json.str origin.reason);
      ("found", Json.option (fun n -> Json.Number n) found);
    ]

let apply rules (inventory : Inventory.t) =
  let records = ref inventory.records and contexts = ref inventory.contexts and removed = ref [] in
  let remove group =
    records := List.filter (fun (r : Inventory.record) -> not (has_all group r.values)) !records;
    contexts := List.filter (fun values -> not (has_all group values)) !contexts
  in
  let* _ =
    read_list
      (function
        | Artifact { product; values; origin } ->
            let hit, kept =
              List.partition
                (fun (r : Inventory.record) -> r.product = product && same_values r.values values)
                !records
            in
            if hit = [] then error (Problem.Exclusion_matches_nothing origin.text)
            else (
              records := kept;
              removed := removal (Some product) values origin None :: !removed;
              Ok ())
        | Group { values; origin } ->
            if
              List.exists (fun (r : Inventory.record) -> has_all values r.values) !records
              || List.exists (has_all values) !contexts
            then (
              remove values;
              removed := removal None values origin None :: !removed;
              Ok ())
            else error (Problem.Exclusion_matches_nothing origin.text)
        | Where _ -> Ok ())
      rules.exclusions
  in
  List.iter
    (fun (preference : preference) ->
      let identities =
        List.sort_uniq compare
          (List.map
             (fun (r : Inventory.record) -> (r.product, List.sort compare r.values))
             !records)
      in
      List.iter
        (fun (product, values) ->
          let copies =
            List.filter
              (fun (r : Inventory.record) -> r.product = product && same_values r.values values)
              !records
          in
          let preferred =
            List.filter
              (fun (r : Inventory.record) ->
                Option.fold ~none:false ~some:(matches_glob preference.glob) r.path)
              copies
          in
          match preferred with
          | [ chosen ] when List.length copies > 1 ->
              records :=
                List.filter
                  (fun (r : Inventory.record) ->
                    r == chosen || not (r.product = product && same_values r.values values))
                  !records;
              removed := removal (Some product) values preference.origin None :: !removed
          | _ -> ())
        identities)
    rules.preferences;
  let judged =
    List.concat_map
      (function
        | Where { dims; condition; origin } ->
            List.filter_map
              (fun group ->
                match evaluate condition group !records !contexts with
                | true, found -> Some (group, origin, found)
                | false, _ -> None)
              (domain dims !records !contexts)
        | Artifact _ | Group _ -> [])
      rules.exclusions
  in
  let seen = ref [] in
  List.iter
    (fun (group, origin, found) ->
      if not (List.mem group !seen) then (
        seen := group :: !seen;
        removed := removal None group origin found :: !removed))
    judged;
  List.iter remove !seen;
  let+ _ =
    read_list
      (fun requirement ->
        read_list
          (fun group ->
            match evaluate requirement.condition group !records !contexts with
            | true, _ -> Ok ()
            | false, found ->
                error
                  (Problem.Requirement_failed
                     {
                       rule = requirement.origin.text;
                       group = String.concat "," (List.map (fun (k, v) -> k ^ "=" ^ v) group);
                       found = Option.fold ~none:"none" ~some:string_of_int found;
                     }))
          (domain requirement.dims !records !contexts))
      rules.requirements
  in
  ({ inventory with records = !records; contexts = !contexts }, List.rev !removed)
