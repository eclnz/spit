open Ast
open Data

let validate_part (test : test) = function
  | Text _ | Expand { value = Builtin "path"; _ } -> Ok ()
  | Expand { value = Port name; _ } when List.mem name test.params -> Ok ()
  | Expand part -> error ~span:part.span test.name "unknown check parameter or placeholder"

let validate_script (test : test) =
  match test.script.commands with
  | [ command ] when command.words <> [] ->
      let* _ =
        read_list
          (fun (word : command_word) ->
            if word.role <> Argument then error test.name "checks cannot redirect streams"
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
      else error test.name "a check must inspect {@path}"
  | _ -> error test.name "a check needs one command"

let collect gates =
  let table = Hashtbl.create 16 in
  let+ _ =
    read_list
      (function
        | Check test ->
            if Hashtbl.mem table test.name then error test.name "duplicate check"
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
  | None -> error name "unknown check"
  | Some test when List.length test.params <> List.length used.args ->
      error name "wrong number of check arguments"
  | Some _ -> Ok ()

let validate gates settings =
  let* definitions = collect gates in
  let+ _ =
    read_list
      (fun (setting : Setting.t located) ->
        match setting.value with
        | Gate (Gate_kind.Check, _, uses) ->
            let+ _ = read_list (validate_use definitions) uses in
            ()
        | _ -> Ok ())
      settings
  in
  ()
