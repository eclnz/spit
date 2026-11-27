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
              | _ -> error "scalar" "bounds must be finite numbers"
            in
            let* target =
              match name with
              | "min" -> Ok minimum
              | "max" -> Ok maximum
              | _ -> error name "unknown scalar bound"
            in
            if !target <> None then error name "scalar bound is repeated"
            else (
              target := Some number;
              Ok ())
        | _ -> error "scalar" "a bound is written min=number or max=number")
      values
  in
  match (!minimum, !maximum) with
  | Some a, Some b when a > b -> error "scalar" "minimum exceeds maximum"
  | _ -> Ok { minimum = !minimum; maximum = !maximum }
