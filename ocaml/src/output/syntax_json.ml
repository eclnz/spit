open Ast
open Json

let reference (r : reference) =
  Text (match r.namespace with None -> r.name | Some ns -> ns ^ "." ^ r.name)

let rec typ = function
  | Var name -> Object [ ("var", str name) ]
  | Named (name, args) -> Object [ ("name", reference name); ("args", list typ args) ]

let extension = function
  | Replace value -> Object [ ("replace", str value) ]
  | Append value -> Object [ ("append", str value) ]

let beside b = Object [ ("of", str b.of_); ("suffix", option str b.suffix) ]

let placement = function
  | Dims dims -> Object [ ("dims", strings dims) ]
  | Here -> str "here"
  | Beside b -> Object [ ("beside", beside b) ]

let shape = function
  | File ext -> Object [ ("file", option extension ext) ]
  | Folder -> str "folder"
  | Folder_from name -> Object [ ("folder_from", str name) ]
  | View -> str "view"

let gate_use g =
  Object [ ("gate", reference g.gate); ("args", strings g.args); ("negated", Bool g.negated) ]

let script_lines (script : script) = Source.script_lines script.span script.indent

let test (t : test) =
  Object
    [
      ("name", str t.name); ("params", strings t.params); ("script", strings (script_lines t.script));
    ]

let gate = function
  | Check t -> Object [ ("check", test t) ]
  | Screen t -> Object [ ("screen", test t) ]
  | Review n -> Object [ ("review", str n) ]

let param (p : param) =
  Object
    [
      ("name", str p.name);
      ("many", Bool p.many);
      ("type", option typ p.typ);
      ("constraints", list (function Min n -> Object [ ("min", Number n) ]) p.constraints);
    ]

let output (o : output) =
  Object
    [
      ("name", option str o.name);
      ("type", option typ o.typ);
      ("shape", shape o.shape);
      ("beside", option beside o.beside);
    ]

let pairs values = Object (List.map (fun (k, v) -> (k, str v)) values)

let iteration = function
  | Vary d -> Object [ ("vary", strings d) ]
  | Each d -> Object [ ("each", strings d) ]
  | Same d -> Object [ ("same", strings d) ]
  | Where p -> Object [ ("where", pairs p) ]
  | First d -> Object [ ("first", str d) ]
  | Last d -> Object [ ("last", str d) ]

let arg a =
  Object
    [
      ("artifact", reference a.artifact);
      ("iterations", list (fun (i : iteration located) -> iteration i.value) a.iterations);
    ]

let call c = Object [ ("callee", reference c.callee); ("args", list arg c.args) ]

let declared (d : declared) = Object [ ("type", option typ d.typ); ("dims", option strings d.dims) ]

let binding b =
  Object
    [ ("names", strings b.names); ("call", call b.call); ("declared", option declared b.declared) ]

let operation (o : operation_decl) =
  Object
    [
      ("name", str o.name);
      ("params", list param o.params);
      ("outputs", list output o.outputs);
      ("body", option (list binding) o.body);
    ]

let artifact (a : artifact) =
  let origin =
    match a.origin with
    | Found f ->
        Object
          [
            ( "found",
              Object
                [
                  ("type", option typ f.typ);
                  ("shape", shape f.shape);
                  ("placement", placement f.placement);
                ] );
          ]
    | Made (job, output, d) ->
        Object
          [
            ( "made",
              Object
                [ ("job", Number job); ("output", Number output); ("declared", option declared d) ]
            );
          ]
  in
  Object [ ("name", str a.name); ("origin", origin) ]

let operation_target = function
  | Every_operation -> str "every_operation"
  | Named_operation n -> Object [ ("operation", str n) ]

let artifact_target = function
  | Every_artifact -> str "every_artifact"
  | Named_artifact n -> Object [ ("artifact", str n) ]
  | Output_of (op, name) -> Object [ ("operation", str op); ("output", option str name) ]
  | Input_of (op, name) -> Object [ ("operation", str op); ("input", str name) ]
  | Extension ext -> Object [ ("extension", str ext) ]

let dimension_target = function
  | Every_dimension -> str "every_dimension"
  | Named_dimension n -> Object [ ("dimension", str n) ]

let setting s =
  let result word target value =
    Object [ ("kind", str word); ("target", target); ("value", value) ]
  in
  match s with
  | Setting.Command (t, v) -> result "command" (operation_target t) (strings (script_lines v))
  | Verify (t, v) -> result "verify" (operation_target t) (strings (script_lines v))
  | Calculate (t, v) -> result "calculate" (operation_target t) (str v.value)
  | Resources (t, v) -> result "resources" (operation_target t) (pairs v)
  | Uses (t, v) ->
      result "uses" (operation_target t)
        (list (fun u -> Object [ ("tool", str u.tool); ("used", Bool u.used) ]) v)
  | Anchored t -> result "anchored" (operation_target t) Null
  | Path (t, v) -> result "path" (artifact_target t) (str (Span.text v.span))
  | Ext (t, v) -> result "ext" (artifact_target t) (extension v)
  | Entities (t, Format v) ->
      result "entities" (dimension_target t) (Object [ ("format", str (Span.text v.span)) ])
  | Entities (t, Rename v) -> result "entities" (dimension_target t) (Object [ ("rename", str v) ])
  | Gate (k, t, v) -> result (gate_word k) (artifact_target t) (list gate_use v)

let rec scope s =
  Object
    [
      ("operations", list operation s.operations);
      ("artifacts", list artifact s.artifacts);
      ("jobs", list call s.jobs);
      ("settings", list (fun (s : Setting.t located) -> setting s.value) s.settings);
      ( "stages",
        list (fun (s : stage) -> Object [ ("name", str s.name); ("scope", scope s.scope) ]) s.stages
      );
    ]

let pipeline p =
  Object
    [
      ("types", option (function Standard -> str "standard" | Strict -> str "strict") p.types);
      ("root", option str p.root);
      ("output_dir", option str p.output_dir);
      ( "imports",
        list
          (fun (u : use_decl) ->
            Object
              [
                ("names", option strings u.names);
                ("from", str u.from);
                ("alias", option str u.alias);
              ])
          p.imports );
      ( "type_defs",
        list
          (fun (d : type_def) -> Object [ ("name", str d.name); ("params", strings d.params) ])
          p.type_defs );
      ("dimensions", strings p.dimensions);
      ("tools", strings p.tools);
      ("gates", list gate p.gates);
      ("global", scope p.global);
    ]

let diagnostic source (d : diagnostic) =
  let line, column = Source.position source d.span.start in
  Object
    [
      ("start", Number d.span.start);
      ("finish", Number d.span.finish);
      ("line", Number line);
      ("column", Number column);
      ("message", str (Diagnostics.render_problem d.problem));
    ]

let result ~source (p, diagnostics) =
  Object
    [
      ("pipeline", pipeline p); ("diagnostics", list (diagnostic (Source.index source)) diagnostics);
    ]
