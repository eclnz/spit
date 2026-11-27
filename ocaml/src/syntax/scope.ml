open Ast

let empty = { operations = []; artifacts = []; jobs = []; settings = []; stages = [] }

let rebase_job offset (artifact : artifact) =
  match artifact.origin with
  | Found _ -> artifact
  | Made (job, output, declared) -> { artifact with origin = Made (job + offset, output, declared) }

let rec stages values =
  let groups = Hashtbl.create 16 and order = ref [] in
  List.iter
    (fun (stage : stage) ->
      match Hashtbl.find_opt groups stage.name with
      | Some chunks -> chunks := stage.scope :: !chunks
      | None ->
          Hashtbl.add groups stage.name (ref [ stage.scope ]);
          order := stage.name :: !order)
    values;
  List.map
    (fun name ->
      let scopes = List.rev !(Hashtbl.find groups name) in
      { name; scope = (match scopes with [ scope ] -> scope | _ -> merge scopes) })
    (List.rev !order)

and merge scopes =
  let offset = ref 0 in
  let combined =
    List.fold_left
      (fun result scope ->
        let artifacts =
          List.fold_left
            (fun acc artifact -> rebase_job !offset artifact :: acc)
            result.artifacts scope.artifacts
        in
        offset := !offset + List.length scope.jobs;
        {
          operations = List.rev_append scope.operations result.operations;
          artifacts;
          jobs = List.rev_append scope.jobs result.jobs;
          settings = List.rev_append scope.settings result.settings;
          stages = List.rev_append scope.stages result.stages;
        })
      empty scopes
  in
  finish_reversed combined

and finish_reversed scope =
  {
    operations = List.rev scope.operations;
    artifacts = List.rev scope.artifacts;
    jobs = List.rev scope.jobs;
    settings = List.rev scope.settings;
    stages = stages (List.rev scope.stages);
  }

let prepend_binding job scope (binding : binding) =
  let artifacts =
    List.mapi
      (fun output (name, span) -> { span; name; origin = Made (job, output, binding.declared) })
      (List.combine binding.names binding.name_spans)
  in
  {
    scope with
    jobs = binding.call :: scope.jobs;
    artifacts = List.rev_append artifacts scope.artifacts;
  }

let of_bindings ~operations ~sources ~settings ~stages bindings =
  let scope = { operations; artifacts = List.rev sources; jobs = []; settings; stages } in
  let _, scope =
    List.fold_left
      (fun (job, scope) binding -> (job + 1, prepend_binding job scope binding))
      (0, scope) bindings
  in
  { scope with artifacts = List.rev scope.artifacts; jobs = List.rev scope.jobs }
