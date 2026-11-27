open Ast

let render_problem = function
  | Unclosed_quote c -> Printf.sprintf "the quote `%c` is never closed" c
  | Unexpected_closing c -> Printf.sprintf "unexpected closing bracket `%c`" c
  | Empty_list_item -> "a comma-separated list has an empty item"
  | Nesting_limit -> "syntax may nest at most 128 levels deep"
  | Unexpected_text s -> Printf.sprintf "unexpected text after the expression: `%s`" s
  | Unclosed_bracket -> "a bracket opened here is never closed"
  | Inline_comment -> "a comment goes on a line of its own"
  | Missing_binding_keyword s ->
      Printf.sprintf "a binding starts with `product`: write `product %s`" s
  | Unknown_keyword s -> Printf.sprintf "`%s` does not start with a keyword" s
  | Stage_needs_name -> "`stage` needs a name: write `stage <name>:`"
  | Inline_script_with_body (h, v) ->
      Printf.sprintf "`%s: %s` has lines under it: a script of several lines starts after `%s:`" h v
        h
  | Missing_script (w, h) -> Printf.sprintf "`%s %s` needs a command" w h
  | Unexpected_indent s ->
      Printf.sprintf
        "lines are indented under `%s`, which neither ends with `:` nor leaves a bracket open" s
  | Value_count (s, n) -> Printf.sprintf "`%s` takes one value, not %d" s n
  | Missing_name s -> s ^ " needs a name"
  | Default_used_as_name -> "`default` is a prefix, not a name: write `default <keyword>: <value>`"
  | Reserved_name s -> Printf.sprintf "`%s` is a reserved word, so it can't be a name" s
  | Invalid_name s ->
      Printf.sprintf
        "`%s` is not a name: use letters, digits and `_`, starting with a letter or `_`" s
  | Named_default s ->
      Printf.sprintf "a default applies to everything, so it names nothing: drop `%s`" s
  | Default_target (w, v) -> Printf.sprintf "write `default %s: %s`" w v
  | Missing_separator (s, t) -> Printf.sprintf "expected `%s` in `%s`" s t
  | Missing_delimiters (a, b, s) -> Printf.sprintf "expected `%c ... %c` in `%s`" a b s
  | Folder_extension -> "a folder has no extension: drop the extension, or the `/`"
  | Description_order s ->
      Printf.sprintf
        "`%s` is out of place or repeated: write `Type .ext / [dims] @gates` in that order" s
  | Empty_gate_annotation k -> Printf.sprintf "`@%s` needs at least one gate" (gate_word k)
  | Repeated_gate_annotation k ->
      Printf.sprintf "write `@%s` once, holding every gate" (gate_word k)
  | Contradicted_gate s -> Printf.sprintf "`%s` is both applied and turned off here" s
  | Folder_source s ->
      Printf.sprintf
        "`%s`: a source may be a file or folder; `from` and `view` describe generated outputs" s
  | Output_placement ->
      "an output takes its dimensions from its call, so `[...]` and `here` are for sources"
  | Invalid_minimum s -> Printf.sprintf "`@min` needs a 32-bit integer, found `%s`" s
  | Invalid_parameter_annotation s -> Printf.sprintf "`%s` is not a parameter annotation" s
  | Invalid_argument_annotation s -> Printf.sprintf "`%s` is not an argument annotation" s
  | Invalid_argument s -> Printf.sprintf "cannot read the argument `%s`" s
  | Binding_description s ->
      Printf.sprintf "a binding declares only its type and dimensions, not `%s`" s
  | Expected_value_only (w, _) ->
      Printf.sprintf "`%s` takes a value and no name: write `%s: <value>`" w w
  | Expected_name_only (w, v) -> Printf.sprintf "`%s` takes a name and no value: drop `: %s`" w v
  | Expected_name w -> Printf.sprintf "`%s` needs a name" w
  | Missing_import_source s | Invalid_import s ->
      Printf.sprintf "write `use [alias:] path [names]`, not `%s`" s
  | Type_parameter_needs_variable -> "a type's parameters are variables, written `$Name`"
  | Invalid_type_name s ->
      Printf.sprintf "`%s` is not a type name: its final name must start with a capital letter" s
  | Defined_type_variable s -> Printf.sprintf "`$%s` is a variable, so it cannot be defined" s
  | Gate_needs_script (k, n, _) -> Printf.sprintf "the %s `%s` needs a command" (gate_word k) n
  | Review_parameters n -> Printf.sprintf "the review `%s` takes no parameters" n
  | Review_script (n, _) ->
      Printf.sprintf "the review `%s` has no command, since a person gives its verdicts" n
  | Disallowed_default w -> Printf.sprintf "`%s` cannot have a default" w
  | Expected_stage_body -> "write `stage <name>:` and indent what it holds"
  | Unexpected_body_value (h, v) ->
      Printf.sprintf "`%s` holds indented lines, so its line ends with `:` (found `%s`)" h v
  | Nested_definition -> "this definition belongs at the top of the file"
  | Explicit_call_required ->
      "a product must call an operation explicitly, including `name()` for no arguments"
  | Composite_statement -> "an operation body contains only `product` bindings"
  | Composite_output -> "a composite operation names its outputs and gives only their types"
  | Duplicate_setting s -> Printf.sprintf "`%s` is set more than once in this scope" s
  | Invalid_extension s -> Printf.sprintf "`%s` is not an extension: write `.ext` or `+.ext`" s
  | Invalid_pattern s -> "invalid pattern: " ^ s
  | Invalid_types_mode s -> Printf.sprintf "`%s` is not a type mode: write `standard` or `strict`" s
  | Empty_value -> "a value must not be empty"
  | Invalid_placeholder s ->
      Printf.sprintf
        "invalid placeholder `%s`: use `{port}`, `{output$dir}`, `{input[dimension]}` or \
         `{@function(...)}`"
        s
  | Shell_operator -> "shell operators require an explicit shell command"
  | Legacy_expansion s -> Printf.sprintf "enclose the SPIT expansion in braces: `{%s}`" s
  | Repeated_iteration s -> Printf.sprintf "`@%s` is repeated or conflicts with another selector" s
