open Data

type t =
  | Help
  | Version
  | Check of string
  | Dag of string * string * string option
  | Recipe of string * string option
  | Pipeline of string * string option * string option * string option

let usage =
  "spit check PIPELINE\n\
   spit dag RECIPE.spitin [--json] [-o FILE]\n\
   spit dag PIPELINE.spit [--root DIR] [--output-root DIR] [--json] [-o FILE]\n\
   spit dag PIPELINE INVENTORY [--json] [-o FILE]\n\
   spit --version\n\
   spit --help"

let rec dag_options output = function
  | [] -> Ok output
  | "--json" :: rest -> dag_options output rest
  | ("-o" | "--output") :: path :: rest when output = None -> dag_options (Some path) rest
  | _ -> error (Problem.Usage usage)

let rec pipeline_options root output_root output = function
  | [] -> Ok (root, output_root, output)
  | "--json" :: rest -> pipeline_options root output_root output rest
  | "--root" :: path :: rest when root = None ->
      pipeline_options (Some path) output_root output rest
  | "--output-root" :: path :: rest when output_root = None ->
      pipeline_options root (Some path) output rest
  | ("-o" | "--output") :: path :: rest when output = None ->
      pipeline_options root output_root (Some path) rest
  | _ -> error (Problem.Usage usage)

let parse = function
  | [ "--help" ] | [ "-h" ] | [] -> Ok Help
  | [ "--version" ] | [ "-V" ] -> Ok Version
  | [ "check"; path ] -> Ok (Check path)
  | "dag" :: recipe :: rest when Filename.check_suffix recipe ".spitin" ->
      let+ output = dag_options None rest in
      Recipe (recipe, output)
  | "dag" :: pipeline :: inventory :: rest when not (String.starts_with ~prefix:"-" inventory) ->
      let+ output = dag_options None rest in
      Dag (pipeline, inventory, output)
  | "dag" :: pipeline :: rest when Filename.check_suffix pipeline ".spit" ->
      let+ root, output_root, output = pipeline_options None None None rest in
      Pipeline (pipeline, root, output_root, output)
  | _ -> error (Problem.Usage usage)
