open Ast
open Data

let validate_part (test : test) = function
  | Text _ | Expand { value = Builtin "path"; _ } -> Ok ()
  | Expand { value = Port name; _ } when List.mem name test.params -> Ok ()
  | Expand part -> error ~span:part.span (Problem.Check_parameter_unknown test.name)

let validate_script (test : test) =
  match test.script.commands with
  | [ command ] when command.words <> [] ->
      let* _ =
        read_list
          (fun (word : command_word) ->
            if word.role <> Argument then error (Problem.Check_redirect test.name)
            else
              let+ _ = read_list (validate_part test) word.parts in
              ())
          command.words
      in
      if
        List.exists
          (fun word ->
            List.exists
              (function Expand { value = Builtin "path"; _ } -> true | _ -> false)
              word.parts)
          command.words
      then Ok ()
      else error (Problem.Check_path test.name)
  | _ -> error (Problem.Check_one_command test.name)

let collect ?(kind = Gate_kind.Check) gates =
  let table = Hashtbl.create 16 in
  let+ _ =
    read_list
      (fun gate ->
        match (kind, gate) with
        | Gate_kind.Check, Check test | Gate_kind.Screen, Screen test ->
            if Hashtbl.mem table test.name then error (Problem.Check_duplicate test.name)
            else
              let+ () = validate_script test in
              Hashtbl.add table test.name test
        | _ -> Ok ())
      gates
  in
  table

let validate_use (definitions : (string, test) Hashtbl.t) (used : gate_use) =
  let name = ref_name used.gate in
  match Hashtbl.find_opt definitions name with
  | None -> error (Problem.Check_unknown name)
  | Some test when List.length test.params <> List.length used.args ->
      error (Problem.Check_arguments name)
  | Some _ -> Ok ()

let validate gates settings =
  let* definitions = collect gates in
  let* screens = collect ~kind:Gate_kind.Screen gates in
  let+ _ =
    read_list
      (fun (setting : Setting.t located) ->
        match setting.value with
        | Gate (Gate_kind.Check, _, uses) ->
            let+ _ = read_list (validate_use definitions) uses in
            ()
        | Gate (Gate_kind.Screen, _, uses) ->
            let+ _ =
              read_list
                (fun (used : gate_use) ->
                  let name = ref_name used.gate in
                  match Hashtbl.find_opt screens name with
                  | None -> error (Problem.Screen_unknown name)
                  | Some test when List.length test.params <> List.length used.args ->
                      error (Problem.Check_arguments name)
                  | Some _ -> Ok ())
                uses
            in
            ()
        | Gate (Gate_kind.Review, _, uses) ->
            let reviews = List.filter_map (function Review name -> Some name | _ -> None) gates in
            let+ _ =
              read_list
                (fun (used : gate_use) ->
                  let name = ref_name used.gate in
                  if not (List.mem name reviews) then error (Problem.Review_unknown name)
                  else if used.args <> [] then error (Problem.Review_arguments name)
                  else Ok ())
                uses
            in
            ()
        | _ -> Ok ())
      settings
  in
  ()
