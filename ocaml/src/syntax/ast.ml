module Keyword = struct
  type t =
    | Types
    | Type
    | Use
    | Root
    | Output
    | Dimensions
    | Source
    | Path
    | Check
    | Screen
    | Review
    | Tool
    | Command
    | Verify
    | Calculate
    | Resources
    | Uses
    | Anchored
    | Stage
    | Product
    | Operation
    | Entities
    | Ext
end

module Gate_kind = struct
  type t = Check | Screen | Review
end

type span = { source : string; start : int; finish : int }

type 'a located = { span : span; value : 'a }

type reference = { namespace : string option; name : string }

type type_expr = Named of reference * type_expr list | Var of string

type type_def = { span : span; name : string; params : string list }

type types_mode = Standard | Strict

type use_decl = { span : span; names : string list option; from : string; alias : string option }

type expansion =
  | Port of string
  | Builtin of string
  | Property of string * string
  | Dimension of string * string
  | Function of string * string list

type command_part = Text of string located | Expand of expansion located

type word_role = Argument | Input_redirect | Output_redirect

type command_word = { span : span; parts : command_part list; quoted : bool; role : word_role }

type command = { span : span; words : command_word list }

type script = { span : span; indent : int; commands : command list }

type test = { name : string; params : string list; script : script }

type gate_def = Check of test | Screen of test | Review of string

type gate_use = { gate : reference; args : string list; negated : bool }

type beside = { of_ : string; suffix : string option }

type placement = Dims of string list | Beside of beside | Here

type extension = Replace of string | Append of string

type shape = File of extension option | Folder | Folder_from of string | View

type constraint_ = Min of int

type param = {
  span : span;
  name : string;
  many : bool;
  typ : type_expr option;
  constraints : constraint_ list;
}

type output = {
  span : span;
  name : string option;
  typ : type_expr option;
  shape : shape;
  beside : beside option;
}

type iteration =
  | Vary of string list
  | Each of string list
  | Same of string list
  | Where of (string * string) list
  | First of string
  | Last of string

type arg = {
  span : span;
  artifact_span : span;
  artifact : reference;
  iterations : iteration located list;
}

type call = { span : span; callee_span : span; callee : reference; args : arg list }

type declared = { typ : type_expr option; dims : string list option }

type found = { typ : type_expr option; shape : shape; placement : placement }

type origin = Found of found | Made of int * int * declared option

type artifact = { span : span; name : string; origin : origin }

type operation_target = Every_operation | Named_operation of string

type artifact_target =
  | Every_artifact
  | Named_artifact of string
  | Output_of of string * string option
  | Input_of of string * string
  | Extension of string

type dimension_target = Every_dimension | Named_dimension of string

type tool_use = { tool : string; used : bool }

type value_shape = Digits | Year | Date

type pattern_value =
  | Product_name
  | Stage_name
  | Entity_names
  | Dimension_name of string * value_shape option
  | Entity_key
  | Entity_value

type pattern_part =
  | Pattern_text of string located
  | Pattern_value of pattern_value located
  | Pattern_optional of pattern_part list

type pattern = { span : span; segments : pattern_part list; suffix : string option }

type entity_format = { span : span; pattern : pattern; separator : string }

type entity_setting = Format of entity_format | Rename of string

module Setting = struct
  type t =
    | Command of operation_target * script
    | Verify of operation_target * script
    | Calculate of operation_target * string located
    | Resources of operation_target * (string * string) list
    | Uses of operation_target * tool_use list
    | Anchored of operation_target
    | Path of artifact_target * pattern
    | Gate of Gate_kind.t * artifact_target * gate_use list
    | Ext of artifact_target * extension
    | Entities of dimension_target * entity_setting
end

type binding = {
  span : span;
  name_spans : span list;
  names : string list;
  call : call;
  declared : declared option;
}

type operation_decl = {
  span : span;
  name : string;
  params : param list;
  outputs : output list;
  body : binding list option;
}

type scope = {
  operations : operation_decl list;
  artifacts : artifact list;
  jobs : call list;
  settings : Setting.t located list;
  stages : stage list;
}

and stage = { name : string; scope : scope }

type pipeline = {
  types : types_mode option;
  root : string option;
  output_dir : string option;
  imports : use_decl list;
  type_defs : type_def list;
  dimensions : string list;
  tools : string list;
  gates : gate_def list;
  global : scope;
}

type entry_form = Plain | Script | Gate of Gate_kind.t

type keyword_rule = {
  keyword : Keyword.t;
  word : string;
  form : entry_form;
  allows_default : bool;
  top_level : bool;
  singleton : bool;
}

let keywords =
  let definition ?(singleton = false) keyword word form =
    { keyword; word; form; allows_default = false; top_level = true; singleton }
  in
  let scoped keyword word form allows_default =
    { keyword; word; form; allows_default; top_level = false; singleton = false }
  in
  let gate keyword word kind =
    { (definition keyword word (Gate kind)) with allows_default = true }
  in
  [
    definition ~singleton:true Types "types" Plain;
    definition Type "type" Plain;
    definition Use "use" Plain;
    definition ~singleton:true Root "root" Plain;
    definition ~singleton:true Output "output" Plain;
    definition ~singleton:true Dimensions "dimensions" Plain;
    definition Tool "tool" Plain;
    gate Check "check" Gate_kind.Check;
    gate Screen "screen" Gate_kind.Screen;
    gate Review "review" Gate_kind.Review;
    definition Source "source" Plain;
    scoped Path "path" Plain true;
    scoped Command "command" Script false;
    scoped Verify "verify" Script true;
    scoped Calculate "calculate" Plain true;
    scoped Resources "resources" Plain true;
    scoped Uses "uses" Plain true;
    scoped Anchored "anchored" Plain true;
    scoped Stage "stage" Plain false;
    scoped Product "product" Plain false;
    scoped Operation "operation" Plain false;
    scoped Entities "entities" Plain true;
    scoped Ext "ext" Plain true;
  ]

let keyword word = List.find_opt (fun rule -> rule.word = word) keywords

let gate_word = function Gate_kind.Check -> "check" | Screen -> "screen" | Review -> "review"

type problem =
  | Unclosed_quote of char
  | Unexpected_closing of char
  | Empty_list_item
  | Nesting_limit
  | Unexpected_text of string
  | Unclosed_bracket
  | Inline_comment
  | Missing_binding_keyword of string
  | Unknown_keyword of string
  | Stage_needs_name
  | Inline_script_with_body of string * string
  | Missing_script of string * string
  | Unexpected_indent of string
  | Value_count of string * int
  | Missing_name of string
  | Default_used_as_name
  | Reserved_name of string
  | Invalid_name of string
  | Named_default of string
  | Default_target of string * string
  | Missing_separator of string * string
  | Missing_delimiters of char * char * string
  | Folder_extension
  | Description_order of string
  | Empty_gate_annotation of Gate_kind.t
  | Repeated_gate_annotation of Gate_kind.t
  | Contradicted_gate of string
  | Folder_source of string
  | Output_placement
  | Invalid_minimum of string
  | Invalid_parameter_annotation of string
  | Invalid_argument_annotation of string
  | Invalid_argument of string
  | Binding_description of string
  | Expected_value_only of string * string
  | Expected_name_only of string * string
  | Expected_name of string
  | Missing_import_source of string
  | Invalid_import of string
  | Type_parameter_needs_variable
  | Invalid_type_name of string
  | Defined_type_variable of string
  | Gate_needs_script of Gate_kind.t * string * string
  | Review_parameters of string
  | Review_script of string * string
  | Disallowed_default of string
  | Expected_stage_body
  | Unexpected_body_value of string * string
  | Nested_definition
  | Explicit_call_required
  | Composite_statement
  | Composite_output
  | Duplicate_setting of string
  | Invalid_extension of string
  | Invalid_pattern of string
  | Invalid_types_mode of string
  | Empty_value
  | Invalid_placeholder of string
  | Shell_operator
  | Repeated_iteration of string
  | Repeated_minimum
  | Keyword_as_name of string
  | Unknown_shape of { shape : string; placeholder : string }
  | Builtin_shape
  | Unknown_builtin of string
  | Builtin_needs_at of string
  | Entities_placeholder of string
  | Group_without_placeholder
  | Entities_order
  | Entities_adjacent
  | Entities_separator
  | Entities_one_separator

type diagnostic = { span : span; problem : problem }

type line = { indent : int; span : span }

type entry_header = { rule : keyword_rule; default : bool; rest : span }

type content = Bare | Value of span | Script_body of span * int | Body of line list

type gate_annotation = { span : span; kind : Gate_kind.t; uses : gate_use list }

type description = {
  typ : type_expr option;
  shape : shape;
  placement : placement option;
  gates : gate_annotation list;
}

type 'a annotated = { value : 'a; gates : gate_annotation list }

type parsed_operation = {
  span : span;
  name : string;
  params : param annotated list;
  outputs : output annotated list;
  body : parsed_scope option;
}

and parsed_scope = { entries : (statement located, diagnostic) result list }

and statement =
  | Types of types_mode
  | Root of string
  | Output_dir of string
  | Import of use_decl
  | Type_def of type_def
  | Dimensions of string list
  | Tool of string
  | Gate_def of gate_def
  | Setting of Setting.t located
  | Source_declaration of artifact annotated
  | Operation_declaration of parsed_operation
  | Product_binding of binding
  | Stage_declaration of string * parsed_scope

type template_part = Literal of span | Expansion of expansion located

type header_syntax =
  | Plain_header
  | Source_header of artifact annotated
  | Source_body_header of span * string
  | Product_header of binding
  | Operation_header of parsed_operation
  | Type_header of type_def

type entry = { header : entry_header; head : span; content : content; syntax : header_syntax }

type header_result = { syntax : header_syntax; head : span; value : span option; rest : span }
