open Ast

let name = function Digits -> "digits" | Year -> "year" | Date -> "date"

let of_name = function
  | "digits" -> Some Digits
  | "year" -> Some Year
  | "date" -> Some Date
  | _ -> None

let fixed_length = function Digits -> None | Year -> Some 4 | Date -> Some 10

let digits text = text <> "" && String.for_all (fun c -> c >= '0' && c <= '9') text

let year text =
  String.length text = 4
  && digits text
  &&
  let value = int_of_string text in
  value >= 1900 && value <= 2099

let days_in year month =
  match month with
  | 2 -> if (year mod 4 = 0 && year mod 100 <> 0) || year mod 400 = 0 then 29 else 28
  | 4 | 6 | 9 | 11 -> 30
  | _ -> 31

let date text =
  String.length text = 10
  && text.[4] = '-'
  && text.[7] = '-'
  &&
  let part start length = String.sub text start length in
  year (part 0 4)
  && digits (part 5 2)
  && digits (part 8 2)
  &&
  let month = int_of_string (part 5 2) and day = int_of_string (part 8 2) in
  month >= 1 && month <= 12 && day >= 1 && day <= days_in (int_of_string (part 0 4)) month

let accepts = function Digits -> digits | Year -> year | Date -> date
