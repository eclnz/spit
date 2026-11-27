type t =
  | Null
  | Bool of bool
  | Number of int
  | Float of float
  | Text of string
  | List of t list
  | Object of (string * t) list

let str s = Text s

let strings xs = List (List.map str xs)

let list f xs = List (List.map f xs)

let option f = function None -> Null | Some x -> f x

let to_string value =
  let buffer = Buffer.create 256 in
  let quote s =
    Buffer.add_char buffer '"';
    String.iter
      (function
        | '"' -> Buffer.add_string buffer "\\\""
        | '\\' -> Buffer.add_string buffer "\\\\"
        | '\n' -> Buffer.add_string buffer "\\n"
        | '\r' -> Buffer.add_string buffer "\\r"
        | '\t' -> Buffer.add_string buffer "\\t"
        | c when Char.code c < 32 ->
            Buffer.add_string buffer (Printf.sprintf "\\u%04x" (Char.code c))
        | c -> Buffer.add_char buffer c)
      s;
    Buffer.add_char buffer '"'
  in
  let sequence opening closing write values =
    Buffer.add_char buffer opening;
    List.iteri
      (fun i v ->
        if i > 0 then Buffer.add_char buffer ',';
        write v)
      values;
    Buffer.add_char buffer closing
  in
  let rec write = function
    | Null -> Buffer.add_string buffer "null"
    | Bool b -> Buffer.add_string buffer (string_of_bool b)
    | Number n -> Buffer.add_string buffer (string_of_int n)
    | Float value -> Buffer.add_string buffer (Printf.sprintf "%.17g" value)
    | Text s -> quote s
    | List values -> sequence '[' ']' write values
    | Object fields ->
        sequence '{' '}'
          (fun (key, value) ->
            quote key;
            Buffer.add_char buffer ':';
            write value)
          fields
  in
  write value;
  Buffer.contents buffer
