open Data

type bounds = { minimum : float option; maximum : float option }

let options values =
  let minimum = ref None and maximum = ref None in
  let* _ =
    read_list
      (fun text ->
        match String.split_on_char '=' text with
        | [ name; value ] ->
            let name = String.trim name in
            let* number =
              match float_of_string_opt (String.trim value) with
              | Some n when Float.is_finite n -> Ok n
              | _ -> error Problem.Scalar_bound_number
            in
            let* target =
              match name with
              | "min" -> Ok minimum
              | "max" -> Ok maximum
              | _ -> error (Problem.Scalar_bound_unknown name)
            in
            if !target <> None then error (Problem.Scalar_bound_twice name)
            else (
              target := Some number;
              Ok ())
        | _ -> error Problem.Scalar_bound_form)
      values
  in
  match (!minimum, !maximum) with
  | Some a, Some b when a > b -> error Problem.Scalar_bounds_order
  | _ -> Ok { minimum = !minimum; maximum = !maximum }
