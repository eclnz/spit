open Ast
open Data

let rec unify subject substitutions expected actual =
  match (expected, actual) with
  | None, _ | _, None -> Ok ()
  | Some (Var name), Some value -> (
      match Hashtbl.find_opt substitutions name with
      | None ->
          Hashtbl.add substitutions name value;
          Ok ()
      | Some existing -> unify subject (Hashtbl.create 0) (Some existing) (Some value))
  | Some (Named (a, aa)), Some (Named (b, bb)) when a = b && List.length aa = List.length bb ->
      let+ _ =
        read_list (fun (a, b) -> unify subject substitutions (Some a) (Some b)) (List.combine aa bb)
      in
      ()
  | _ -> error subject "input type does not match the operation's signature"

let rec substitute substitutions = function
  | Var name -> Hashtbl.find_opt substitutions name
  | Named (name, args) ->
      let args = List.map (substitute substitutions) args in
      if List.exists Option.is_none args then None
      else Some (Named (name, List.filter_map Fun.id args))

let infer_output substitutions (output : output) = Option.bind output.typ (substitute substitutions)

let check_declared ?(strict = false) definitions subject typ =
  let rec check = function
    | Var _ -> Ok ()
    | Named (name, args) ->
        let* () =
          match Hashtbl.find_opt definitions (ref_name name) with
          | Some count when count <> List.length args ->
              error subject "wrong number of type arguments"
          | None when strict && args <> [] ->
              error subject "strict typing requires a declaration for generic types"
          | _ -> Ok ()
        in
        let+ _ = read_list check args in
        ()
  in
  match typ with None -> Ok () | Some typ -> check typ

let rec has_variable = function Var _ -> true | Named (_, args) -> List.exists has_variable args
