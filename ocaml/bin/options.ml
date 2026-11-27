open Data

type t = Help | Version | Check of string | Dag of string * string * string option

let usage =
  "spit check PIPELINE\nspit dag PIPELINE INVENTORY [--json] [-o FILE]\nspit --version\nspit --help"

let rec dag_options output = function
  | [] -> Ok output
  | "--json" :: rest -> dag_options output rest
  | ("-o" | "--output") :: path :: rest when output = None -> dag_options (Some path) rest
  | _ -> error "usage" usage

let parse = function
  | [ "--help" ] | [ "-h" ] | [] -> Ok Help
  | [ "--version" ] | [ "-V" ] -> Ok Version
  | [ "check"; path ] -> Ok (Check path)
  | "dag" :: pipeline :: inventory :: rest when not (String.starts_with ~prefix:"-" inventory) ->
      let+ output = dag_options None rest in
      Dag (pipeline, inventory, output)
  | _ -> error "usage" usage
