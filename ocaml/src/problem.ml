type extension_writer = Operation_writes of string | Source_declares of string

type t =
  | Usage of string
  | File_failed of string
  | Syntax of string * Ast.problem
  | Not_implemented of string * string
  | Artifact_lacks_dimension of string
  | Inventory_root_twice
  | Inventory_root_empty
  | Inventory_unsupported
  | Directory_twice of string
  | Discovery_needs_root
  | Pipeline_and_recipe of string
  | Recipe_field_twice of string
  | Recipe_field_empty of string
  | Recipe_directive of string
  | Recipe_input_rule
  | Recipe_input_twice of string
  | Recipe_input_empty of string
  | Recipe_indented
  | Recipe_needs_pipeline
  | Recipe_names_no_source of string
  | Recipe_path_twice of string
  | Inventory_unknown_source of string
  | Inventory_not_source of string
  | Inventory_dimensions of string
  | Inventory_duplicate of string
  | Missing_input of { port : string; operation : string; at : string }
  | Ambiguous_input of { port : string; count : int; at : string }
  | Below_minimum of { port : string; at : string; count : int; minimum : int }
  | Parallel_members of { operation : string; holder : string; other : string; member : string }
  | Parallel_order of string
  | Probe_redirect
  | Probe_literal
  | Probe_one_command
  | Tool_entry
  | Tool_empty of string
  | Tools_section
  | Duplicate_tool of string
  | Tool_without_identity of string
  | No_command of string
  | Import_missing of string
  | Import_collision of string
  | Import_cycle of string
  | Read_failed of { path : string; message : string }
  | Verify_names_inputs
  | Property_on_input of string
  | Stem_needs_extension of string
  | Placeholder_unknown of string
  | Output_context
  | Builtin_unknown of string
  | Projection_missing of string
  | Collection_function of string
  | Scalar_input of string
  | Function_unknown
  | Many_whole_argument
  | Verify_redirect
  | Check_parameter_unknown of string
  | Check_redirect of string
  | Check_path of string
  | Check_one_command of string
  | Check_duplicate of string
  | Check_unknown of string
  | Check_arguments of string
  | Check_no_executable of string
  | Check_placeholder
  | Selector_absent of { input : string; dims : string list }
  | Selector_repeated of { input : string; dims : string list }
  | Where_repeated of string
  | Second_pick of string
  | Pinned_used of { input : string; dims : string list }
  | Many_selectors of string
  | Vary_needs_many of string
  | Each_with_same of string
  | Broadcast_on_driver of { driver : string; dim : string }
  | Broadcast_twice of { first : string; second : string; dim : string }
  | Broadcast_restores of { input : string; dim : string; collector : string }
  | Driver_lacks of { input : string; driver : string option; dims : string list }
  | Memory_format
  | Memory_unit
  | Memory_range
  | Setting_repeated of string
  | Cores_value
  | Resource_unknown of string
  | Resource_repeated of string
  | Undeclared_tool
  | Uses_unknown_operation of string
  | Duplicate_name of { subject : string; name : string }
  | Setting_unknown_operation of string
  | Command_and_calculate of string
  | Calculate_and_command of string
  | Calculate_output of string
  | Setting_unknown_product of string
  | Minimum_bound of string
  | Duplicate_port of string
  | Source_type_variable of string
  | Strict_source_type of string
  | Duplicate_source_dimension of string
  | Unknown_input of string
  | Cycle
  | Broadcast_cannot_drive of string
  | Parallel_vary of string
  | Parallel_dimensions of string
  | Declaration_type_variable of string
  | Declared_dimensions of string
  | Strict_port_types of string
  | Unknown_operation of string
  | Argument_count of string
  | Output_count of string
  | Composite_dimensions of string
  | Composite_input of string
  | Composite_assigned_twice of string
  | Composite_unassigned of string
  | Composite_cycle of string
  | Composite_argument_count of string
  | Composite_output_count of string
  | Composite_settings of string
  | Calculation of { operation : string; message : string }
  | Type_mismatch of string
  | Type_arguments of string
  | Strict_generic of string
  | Scalar_bound_number
  | Scalar_bound_unknown of string
  | Scalar_bound_twice of string
  | Scalar_bound_form
  | Scalar_bounds_order
  | Output_port_unknown of string
  | Input_port_unknown of string
  | Job_lacks_dimension of string
  | Placeholder_names_nothing of string
  | Here_outside
  | Extension_differs of string
  | Property_unknown of string
  | Redirect_port
  | Redirect_one_path
  | Redirect_needs_port
  | No_executable
  | Entities_without_format of string
  | Entity_format_invalid of string
  | Path_placeholder_invalid of string
  | Path_value_missing of string
  | Path_escapes_root of string
  | Path_extension_conflict of string
  | Path_collision of { product : string; path : string }
  | Discovery_placeholder of string
  | No_path_rule of string
  | Source_no_path_rule of string
  | Output_no_path_rule of string
  | No_files of string
  | Claimed_twice of { path : string; first : string; second : string }
  | Local_file_missing of string
  | Two_shapes of {
      product : string;
      dim : string;
      first : Ast.value_shape;
      second : Ast.value_shape;
    }
  | Touching_shapes of { product : string; dropped : bool }
  | Touching_values of { product : string; dropped : bool; first : string; second : string }
  | Rule_extension of {
      product : string;
      literal : string;
      writer : extension_writer;
      ext : string;
    }
  | Default_extension_source of { literal : string; writer : extension_writer; ext : string }
  | Default_extension_output of { literal : string; writer : extension_writer; ext : string }
  | Default_extension_setting of { product : string; literal : string; ext : string }
  | Rule_unknown_dimension of { product : string; dim : string }
  | Absent_dimension of { product : string; dim : string; own_rule : bool }
  | Entities_without_dimensions of string
  | Omitted_dimension of { product : string; dim : string }
  | Shape_in_default
  | Shape_on_output of string
  | Same_path of { first : string; second : string; path : string }
  | Label_without_format of string
  | Label_unknown of string
  | Label_is_dimension of { dim : string; label : string }
  | Label_twice of string

let quoted names = String.concat ", " (List.map (fun n -> "`" ^ n ^ "`") names)

let writer = function
  | Operation_writes operation -> Printf.sprintf "operation `%s` writes" operation
  | Source_declares source -> Printf.sprintf "source `%s` declares" source

let dropped product = function
  | true ->
      Printf.sprintf "in the path rule for `%s`, once the groups its dimensions lack are dropped, "
        product
  | false -> ""

let not_implemented feature = feature ^ " not implemented in the compiler experiment"

let parts = function
  | Usage text -> ("usage", text)
  | File_failed message -> ("file", message)
  | Syntax (subject, problem) -> (subject, Diagnostics.render_problem problem)
  | Not_implemented (subject, feature) -> (subject, not_implemented feature)
  | Artifact_lacks_dimension subject -> (subject, "an artifact lacks a required dimension")
  | Inventory_root_twice -> ("inventory", "a root is declared twice")
  | Inventory_root_empty -> ("inventory", "a root needs a directory")
  | Inventory_unsupported ->
      ("inventory", "the experiment accepts roots and a flat sources: section of identities")
  | Directory_twice name -> (name, "directory is declared both in pipeline and on command line")
  | Discovery_needs_root -> ("root", "source discovery needs a root directory")
  | Pipeline_and_recipe name -> ("recipe", name ^ " is declared in both pipeline and recipe")
  | Recipe_field_twice name -> ("recipe", name ^ " is declared twice")
  | Recipe_field_empty name -> ("recipe", name ^ " needs a value")
  | Recipe_directive text -> ("recipe", "unsupported recipe directive: " ^ text)
  | Recipe_input_rule -> ("recipe", "an input rule needs product: pattern")
  | Recipe_input_twice name -> (name, "input path is declared twice")
  | Recipe_input_empty name -> (name, "an input path needs a pattern")
  | Recipe_indented -> ("recipe", "unexpected indented recipe line")
  | Recipe_needs_pipeline -> ("recipe", "a recipe needs pipeline FILE")
  | Recipe_names_no_source name -> (name, "recipe input path must name a source")
  | Recipe_path_twice name -> (name, "source has paths in both pipeline and recipe")
  | Inventory_unknown_source name -> (name, "inventory names an unknown source")
  | Inventory_not_source name -> (name, "inventory may contain only source artifacts")
  | Inventory_dimensions name -> (name, "inventory dimensions differ from the source declaration")
  | Inventory_duplicate name -> (name, "duplicate inventory identity")
  | Missing_input { port; operation; at } ->
      ( port,
        Printf.sprintf "missing input for `%s`%s" operation (if at = "" then "" else " at " ^ at) )
  | Ambiguous_input { port; count; at } ->
      ( port,
        Printf.sprintf "ambiguous input: %d artifacts match%s" count
          (if at = "" then "" else " at " ^ at) )
  | Below_minimum { port; at; count; minimum } ->
      ( port,
        Printf.sprintf "collection%s has %d artifacts, fewer than @min(%d) requires"
          (if at = "" then "" else " at " ^ at)
          count minimum )
  | Parallel_members { operation; holder; other; member } ->
      ( operation,
        Printf.sprintf "parallel collections differ: `%s` has %s and `%s` does not" holder member
          other )
  | Parallel_order operation ->
      (operation, "parallel collections differ: their members are in different orders")
  | Probe_redirect -> ("tool", "a probe runs without shell redirection")
  | Probe_literal -> ("tool", "a probe takes literal arguments")
  | Probe_one_command -> ("tool", "a probe needs one command")
  | Tool_entry -> ("tools", "a tool needs name: IDENTITY or name: probe COMMAND")
  | Tool_empty name -> (name, "a tool needs an identity or probe command")
  | Tools_section -> ("tools", "expected tools: with indented entries")
  | Duplicate_tool name -> (name, "duplicate tool")
  | Tool_without_identity name -> (name, "declared tool has no identity in the tools file")
  | No_command name -> (name, "operation has no command")
  | Import_missing name -> (name, "selected import does not exist")
  | Import_collision name -> (name, "import collides with an existing definition")
  | Import_cycle path -> (path, "import cycle")
  | Read_failed { path; message } -> (path, message)
  | Verify_names_inputs -> ("verify", "verify commands may name only input ports")
  | Property_on_input name -> (name, "path properties belong to output ports")
  | Stem_needs_extension name -> (name, "stem and prefix require a declared extension")
  | Placeholder_unknown name -> (name, "placeholder names no port or job dimension")
  | Output_context -> ("@output", "use the declared output context name")
  | Builtin_unknown name -> (name, "unsupported command built-in")
  | Projection_missing name -> (name, "input lacks the projected dimension")
  | Collection_function name -> (name, "join and list require a many input")
  | Scalar_input name -> (name, "scalar requires one input file")
  | Function_unknown -> ("command", "unknown command function")
  | Many_whole_argument -> ("command", "a many placeholder must fill an unquoted whole argument")
  | Verify_redirect -> ("verify", "verify commands cannot redirect streams")
  | Check_parameter_unknown name -> (name, "unknown check parameter or placeholder")
  | Check_redirect name -> (name, "checks cannot redirect streams")
  | Check_path name -> (name, "a check must inspect {@path}")
  | Check_one_command name -> (name, "a check needs one command")
  | Check_duplicate name -> (name, "duplicate check")
  | Check_unknown name -> (name, "unknown check")
  | Check_arguments name -> (name, "wrong number of check arguments")
  | Check_no_executable name -> (name, "check has no executable")
  | Check_placeholder -> ("check", "invalid check placeholder")
  | Selector_absent { input; dims } -> (input, "the input has no dimension " ^ quoted dims)
  | Selector_repeated { input; dims } -> (input, "selector names " ^ quoted dims ^ " twice")
  | Where_repeated input -> (input, "@where repeats a dimension")
  | Second_pick input ->
      ( input,
        "choose one selector: `@first` or `@last` is already applied to this input, here or by its \
         caller" )
  | Pinned_used { input; dims } ->
      ( input,
        quoted dims ^ " is pinned by @where, so it cannot also be gathered, broadcast or selected"
      )
  | Many_selectors input ->
      (input, "a many input needs @vary and cannot broadcast or use a single-artifact selector")
  | Vary_needs_many input -> (input, "@vary requires a many input")
  | Each_with_same input -> (input, "@each cannot be combined with @same")
  | Broadcast_on_driver { driver; dim } ->
      ("@each", Printf.sprintf "the driving input `%s` already has `%s`" driver dim)
  | Broadcast_twice { first; second; dim } ->
      ("@each", Printf.sprintf "`%s` and `%s` both broadcast `%s`" first second dim)
  | Broadcast_restores { input; dim; collector } ->
      ( "@each",
        Printf.sprintf
          "`%s` broadcasts `%s`, which `%s` collects with @vary; that would restore the dimension"
          input dim collector )
  | Driver_lacks { input; driver; dims } ->
      let driving =
        match driver with
        | Some driver -> Printf.sprintf "the driving input `%s`" driver
        | None -> "the step"
      in
      let dims = String.concat ", " dims in
      ( "matching",
        Printf.sprintf
          "input `%s` has dimensions %s lacks: %s; write `%s @each(%s)` to run the step for each \
           value"
          input driving dims input dims )
  | Memory_format -> ("memory", "memory needs a whole number and K, M, G or T")
  | Memory_unit -> ("memory", "unknown memory unit")
  | Memory_range -> ("memory", "memory must be a nonnegative integer of at most 2^53 bytes")
  | Setting_repeated name -> (name, "setting is repeated")
  | Cores_value -> ("cores", "cores must be a positive integer")
  | Resource_unknown key -> (key, "unknown resource")
  | Resource_repeated key -> (key, "resource key is repeated")
  | Undeclared_tool -> ("uses", "operation names an undeclared tool")
  | Uses_unknown_operation name -> (name, "uses names an unknown operation")
  | Duplicate_name { subject; name } -> (subject, "duplicate name `" ^ name ^ "`")
  | Setting_unknown_operation name -> (name, "setting names an unknown operation")
  | Command_and_calculate name -> (name, "command conflicts with calculate")
  | Calculate_and_command name -> (name, "calculate conflicts with command")
  | Calculate_output name -> (name, "calculate requires one file output")
  | Setting_unknown_product name -> (name, "setting names an unknown product")
  | Minimum_bound name -> (name, "@min requires a positive bound on a many input")
  | Duplicate_port name -> (name, "duplicate input or output port name")
  | Source_type_variable name -> (name, "type variables belong in operation signatures")
  | Strict_source_type name -> (name, "strict typing requires a source type")
  | Duplicate_source_dimension name -> (name, "duplicate source dimension")
  | Unknown_input name -> (name, "unknown input product")
  | Cycle -> ("pipeline", "product dependencies contain a cycle")
  | Broadcast_cannot_drive name -> (name, "a broadcast input cannot drive the call")
  | Parallel_vary name -> (name, "parallel many inputs require the same @vary")
  | Parallel_dimensions name -> (name, "parallel collections require the same dimensions")
  | Declaration_type_variable name -> (name, "a product declaration cannot contain type variables")
  | Declared_dimensions name -> (name, "declared dimensions differ from the call")
  | Strict_port_types name -> (name, "strict typing requires known port and output types")
  | Unknown_operation name -> (name, "unknown operation")
  | Argument_count name -> (name, "wrong number of input arguments")
  | Output_count name -> (name, "wrong number of product names")
  | Composite_dimensions name -> (name, "declared composite dimensions differ from its result")
  | Composite_input name -> (name, "a composite input must be a port or an earlier local product")
  | Composite_assigned_twice name -> (name, "composite product is assigned more than once")
  | Composite_unassigned name -> (name, "composite body does not assign a named output")
  | Composite_cycle name -> (name, "composite operation cycle")
  | Composite_argument_count name -> (name, "wrong number of composite arguments")
  | Composite_output_count name -> (name, "wrong number of composite outputs")
  | Composite_settings name ->
      (name, "a composite operation cannot have its own command, verify, calculate or resources")
  | Calculation { operation; message } -> (operation, message)
  | Type_mismatch name -> (name, "input type does not match the operation's signature")
  | Type_arguments name -> (name, "wrong number of type arguments")
  | Strict_generic name -> (name, "strict typing requires a declaration for generic types")
  | Scalar_bound_number -> ("scalar", "bounds must be finite numbers")
  | Scalar_bound_unknown name -> (name, "unknown scalar bound")
  | Scalar_bound_twice name -> (name, "scalar bound is repeated")
  | Scalar_bound_form -> ("scalar", "a bound is written min=number or max=number")
  | Scalar_bounds_order -> ("scalar", "minimum exceeds maximum")
  | Output_port_unknown name -> (name, "unknown output port")
  | Input_port_unknown name -> (name, "unknown input port")
  | Job_lacks_dimension name -> (name, "job does not have this dimension")
  | Placeholder_names_nothing name -> (name, "placeholder names no port or dimension")
  | Here_outside -> ("@here", "an imported command's script folder lies outside the pipeline folder")
  | Extension_differs name -> (name, "output path differs from its declared extension")
  | Property_unknown name -> (name, "unknown output property")
  | Redirect_port -> ("command", "redirection must name an appropriate port, once")
  | Redirect_one_path -> ("command", "redirection needs one artifact path")
  | Redirect_needs_port -> ("command", "redirection needs a following port")
  | No_executable -> ("command", "command has no executable")
  | Entities_without_format name -> (name, "{@entities} needs a default entities format")
  | Entity_format_invalid name -> (name, "invalid entity format")
  | Path_placeholder_invalid name -> (name, "invalid path placeholder")
  | Path_value_missing name -> (name, "a required path placeholder has no value")
  | Path_escapes_root name -> (name, "artifact path must stay within its root")
  | Path_extension_conflict name -> (name, "path extension conflicts with declared extension")
  | Path_collision { product; path } -> (product, "artifact path collision: " ^ path)
  | Discovery_placeholder name ->
      (name, "source discovery supports literal text and dimension placeholders")
  | No_path_rule name -> (name, "artifact has no path rule")
  | Source_no_path_rule name -> (name, Printf.sprintf "source `%s` has no path rule" name)
  | Output_no_path_rule name ->
      ( name,
        Printf.sprintf
          "output `%s` has no path rule; write `path %s: ...`, or a `default path:` for the \
           pipeline or its stage"
          name name )
  | No_files name -> (name, "source path matched no files")
  | Claimed_twice { path; first; second } ->
      (first, Printf.sprintf "`%s` matches the path rules of both `%s` and `%s`" path first second)
  | Local_file_missing name -> (name, "pipeline-local source file is missing")
  | Two_shapes { product; dim; first; second } ->
      ( product,
        Printf.sprintf "`%s` has two shapes, `%s` and `%s`" dim (Shape.name first)
          (Shape.name second) )
  | Touching_shapes { product; dropped = d } ->
      (product, dropped product d ^ "no text between two shapes of any length")
  | Touching_values { product; dropped = d; first; second } ->
      ( product,
        Printf.sprintf
          "%s`{%s}{%s}` has no text between two values of any length, so a file's name does not \
           say where one ends; put a `-` or `_` between them"
          (dropped product d) first second )
  | Rule_extension { product; literal; writer = w; ext } ->
      ( product,
        Printf.sprintf
          "path `%s` ends in `%s`, but %s `%s`; drop the extension or use `%s`; SPIT reads the \
           extension from the first `.` after the last placeholder, so keep `.` out of the name \
           before it"
          product literal (writer w) ext ext )
  | Default_extension_source { literal; writer = w; ext } ->
      ( "default path",
        Printf.sprintf
          "the default path ends in `%s`, but %s `%s`; write the default path without an \
           extension, so each source's declared extension completes it"
          literal (writer w) ext )
  | Default_extension_output { literal; writer = w; ext } ->
      ( "default path",
        Printf.sprintf
          "the default path ends in `%s`, but %s `%s`; write the default path without an \
           extension, and give the outputs that use it `ext: %s`"
          literal (writer w) ext literal )
  | Default_extension_setting { product; literal; ext } ->
      ( product,
        Printf.sprintf
          "the default path for `%s` ends in `%s`, but `default ext:` sets `%s`; write the \
           extension once, with `default ext:`"
          product literal ext )
  | Rule_unknown_dimension { product; dim } ->
      (product, Printf.sprintf "path rule names `{%s}`, which no product has" dim)
  | Absent_dimension { product; dim; own_rule } ->
      ( product,
        Printf.sprintf
          "path template for `%s` uses absent dimension `%s`; put it in `[...]` if only some \
           products have it%s"
          product dim
          (if own_rule then ""
           else Printf.sprintf ", or give `%s` a rule of its own with `path %s:`" product product)
      )
  | Entities_without_dimensions product ->
      ( product,
        Printf.sprintf
          "path rule for `%s` uses `{@entities}`, but `%s` has no dimensions; put it in `[...]`, \
           as `[{@entities}_]`"
          product product )
  | Omitted_dimension { product; dim } ->
      ( product,
        Printf.sprintf
          "path rule for `%s` omits dimension `%s`, so two of its artifacts could share a path; \
           add `{%s}` or `{@entities}`"
          product dim dim )
  | Shape_in_default ->
      ( "default path",
        "`default path:` is the rule for outputs too; write the shape in a `path` rule for each \
         source, as `path <source>: ...`" )
  | Shape_on_output name ->
      (name, Printf.sprintf "shapes narrow a source's path rule only; `%s` is made by a step" name)
  | Same_path { first; second; path } ->
      ( second,
        Printf.sprintf "products `%s` and `%s` bind to the same path `%s` for the same entities"
          first second path )
  | Label_without_format dim ->
      ( dim,
        "an entity label renames a dimension in a `default entities:` spelling, and there is none"
      )
  | Label_unknown dim -> (dim, Printf.sprintf "an entity label names `%s`, which no product has" dim)
  | Label_is_dimension { dim; label } ->
      ( dim,
        Printf.sprintf
          "the label `%s` is also a dimension, so a file's name would spell more than one \
           dimension the same way"
          label )
  | Label_twice dim -> (dim, Printf.sprintf "`%s` has a duplicate entity label" dim)

let render problem =
  let subject, message = parts problem in
  subject ^ ": " ^ message
