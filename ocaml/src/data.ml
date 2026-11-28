open Ast

module Product_id = struct
  type t = Product_id of int

  let index (Product_id n) = n
end

module Artifact_id = struct
  type t = Artifact_id of int

  let index (Artifact_id n) = n
end

module Job_id = struct
  type t = Job_id of int

  let number (Job_id n) = n
end

module Symbol = struct
  type t = Symbol of int

  let index (Symbol n) = n
end

type error = { span : span option; file : string option; problem : Problem.t }

let error ?span ?file problem = Error { span; file; problem }

let located span = Result.map_error (fun error -> { error with span = Some span })

let ( let* ) = Result.bind

let ( let+ ) value f = Result.map f value

let read_list = Span.read_list

let port_name (output : output) = Option.value output.name ~default:"output"

let ref_name (r : reference) = match r.namespace with None -> r.name | Some n -> n ^ "." ^ r.name

module Table = struct
  type 'a t = { mutable rows : 'a option array; mutable length : int }

  let create () = { rows = Array.make 16 None; length = 0 }

  let add table value =
    if table.length = Array.length table.rows then (
      let rows = Array.make (2 * table.length) None in
      Array.blit table.rows 0 rows 0 table.length;
      table.rows <- rows);
    let index = table.length in
    table.rows.(index) <- Some value;
    table.length <- index + 1;
    index

  let get table index =
    match table.rows.(index) with
    | Some value -> value
    | None -> invalid_arg "an allocated table row is present"

  let to_array table = Array.init table.length (get table)
end

type symbols = { ids : (string, Symbol.t) Hashtbl.t; text : string Table.t }

let symbols () = { ids = Hashtbl.create 64; text = Table.create () }

let intern symbols text =
  match Hashtbl.find_opt symbols.ids text with
  | Some id -> id
  | None ->
      let id = Symbol.Symbol (Table.add symbols.text text) in
      Hashtbl.add symbols.ids text id;
      id

let symbol_text symbols id = Table.get symbols.text (Symbol.index id)

type product = {
  here : bool;
  path_name : string;
  id : Product_id.t;
  name : string;
  typ : type_expr option;
  dims : Symbol.t array;
  shape : shape;
  stage : string list;
  settings : Setting.t located list;
}

type selection = {
  filters : (Symbol.t * Symbol.t) list;
  pick : (Symbol.t * bool) option;
  effective : Symbol.t array;
  matching : Symbol.t array;
  varying : Symbol.t array;
  each : Symbol.t array;
}

type configuration = { cores : int; memory : int option; anchored : bool }

type step = {
  settings : Setting.t located list;
  configuration : configuration;
  calculation : Json.t option;
  selections : selection array;
  operation : operation_decl;
  inputs : Product_id.t array;
  outputs : Product_id.t array;
  args : arg array;
  varying : Symbol.t array;
  driver : int option;
  minimum : int;
  stage : string list;
  commands : command list;
  verify : command list;
}

type composite_contract = {
  span : span;
  operation : operation_decl;
  args : arg list;
  outputs : string list;
  declared : declared option;
}

type program = {
  syntax : Ast.pipeline;
  files : (string * string) list;
  libraries : scope list;
  contracts : composite_contract list;
}

type checked = {
  symbols : symbols;
  products : product array;
  names : (string, Product_id.t) Hashtbl.t;
  sources : Product_id.t array;
  steps : step array;
  pipeline : program;
}

type entities = (Symbol.t * Symbol.t) array

type artifact = { product : Product_id.t; entities : entities; producer : Job_id.t option }

type job = {
  id : Job_id.t;
  step : int;
  inputs : Artifact_id.t array array;
  outputs : Artifact_id.t array;
  entities : entities;
  depends_on : Job_id.t list;
}

type resolved = { checked : checked; artifacts : artifact array; jobs : job array }

let product checked id = checked.products.(Product_id.index id)

let artifact resolved id = resolved.artifacts.(Artifact_id.index id)

let has_dim dims dim = Array.exists (( = ) dim) dims

let project subject dims entities =
  read_list
    (fun dim ->
      match Array.find_opt (fun (key, _) -> key = dim) entities with
      | Some (_, v) -> Ok v
      | None -> error (Problem.Artifact_lacks_dimension subject))
    (Array.to_list dims)

let natural a b =
  let na = String.length a and nb = String.length b in
  let digit c = c >= '0' && c <= '9' in
  let rec compare_at i j =
    if i = na || j = nb then Int.compare (na - i) (nb - j)
    else if digit a.[i] && digit b.[j] then (
      let ai = ref i and bj = ref j in
      while !ai < na && digit a.[!ai] do
        incr ai
      done;
      while !bj < nb && digit b.[!bj] do
        incr bj
      done;
      let az = ref i and bz = ref j in
      while !az < !ai && a.[!az] = '0' do
        incr az
      done;
      while !bz < !bj && b.[!bz] = '0' do
        incr bz
      done;
      let lengths = Int.compare (!ai - !az) (!bj - !bz) in
      let numbers =
        if lengths <> 0 then lengths
        else String.compare (String.sub a !az (!ai - !az)) (String.sub b !bz (!bj - !bz))
      in
      if numbers <> 0 then numbers else compare_at !ai !bj)
    else
      let c = Char.compare a.[i] b.[j] in
      if c = 0 then compare_at (i + 1) (j + 1) else c
  in
  let c = compare_at 0 0 in
  if c = 0 then String.compare a b else c

let compare_values symbols a b =
  let rec loop a b =
    match (a, b) with
    | [], [] -> 0
    | [], _ -> -1
    | _, [] -> 1
    | x :: xs, y :: ys ->
        let c = natural (symbol_text symbols x) (symbol_text symbols y) in
        if c = 0 then loop xs ys else c
  in
  loop a b
