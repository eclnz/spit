# OCaml test port plan

The Rust compiler's tests were removed with it in `ec567af5`. They tested library types (`parse_fixture`, `ResolvedDag`, `Diagnostic`) and the old syntax, so none port as code. This plan lists each one, 917 in all, with what to do with the behaviour it pinned down. Read any of them with `git show dev:tests/<file>.rs` or `git show dev:src/<file>.rs`.

The runner's tests in `runner/tests` and the usability answer keys stay as they are: they use SPIT only as a program and are the acceptance gate for #422. They are not listed here.

## Form of the new tests

Ported tests are OCaml values in `ocaml/tests`, one module per area, such as `resolution_tests.ml` and `matching_tests.ml`. `plan_tests` runs every test in process through `Driver`, the planning functions the command line uses, and reports every failure.

- A test names its sources, its steps and the records of its dataset. `Plan_support` writes each source's path rule and an empty file for each record, so no test needs a `.spitout`.
- Most Rust tests were one pipeline with a line changed. A variant is the base test with `vary`, `replace` or `redim` applied, and `replace` fails if the text it replaces is missing, so a variant cannot go stale unnoticed.
- `plan`, `plan_in` and `check` run the command; `jobs`, `at`, `same` and `fails` say what it gives. `at` compares a path into the DAG, such as `jobs.*.outputs.output.entities`, with compact JSON.
- An error test names the problem it expects: `problem (Absent_dimension { product = "raw"; dim = "id"; own_rule = true })` for an error after parsing, a `Problem.t` from `src/problem.ml`, or `parse (Unclosed_bracket)` for a syntax error, an `Ast.problem`. Tests never match the wording, which `Problem.render` and `Diagnostics.render_problem` own, so a message can change without touching a test. `fails` matches words only for a gap whose problem does not exist yet.

The checks in `compiler.py` and `cli.py` move into these modules as their areas are ported.

## Actions

- `port`: write a case for the behaviour, in the agreed syntax.
- `merge`: the behaviour is covered by a case of a nearby test; check that one covers it and write no case of its own. Unit tests of Rust internals whose behaviour is observable from the command line are merged into the cases of their area.
- `drop`: not carried over. The note says why.
- `bench`: a timing claim. It belongs in `profiling/`, not in a test that can fail on a slow machine. Tests about stack depth stay as `port`: they are correctness.
- `defer`: waits for work excluded from this branch, such as documentation.

A heading gives a file's action; a test whose action differs says so after its name. The actions are judged from the test names and a sample of their bodies. Read a test's body before porting it, and change its action if the body disagrees.

## Counts

- `port`: 650
- `merge`: 127
- `drop`: 121
- `bench`: 18
- `defer`: 1

## Decisions

Settled before porting, and applied to the lists below:

- **The editor contract is dropped.** `check --json`, `--stdin`, `--hovers`, UTF-16 columns, diagnostic codes and the `keywords` table are not carried over, with their tests. `dag --json`, which writes the `.spitdag`, stays. spit-vscode loses its diagnostics and hovers until #423 defines what replaces them, and #423 stays a merge blocker.
- **`artifacts --by-target` stays; `dag --tree` goes.** The by-target trace is the guide's step for finding what a missing file holds up. The text topology renderer and its layout tests are dropped.
- **The `.spitout` goes, both reading and writing** ([#418](https://github.com/eclnz/spit/issues/418)). Tests whose subject is the file, its round trip or its equivalence with a recipe are dropped. Tests that write an inventory only as a way to give a case its sources move to whatever #418 settles for hand-written records: records in a recipe, or folders of empty files. The OCaml compiler's own `dag pipeline.spit inventory.spitout` form and the cases in `ocaml/tests/compiler.py` that use it go the same way. #418 also leaves open whether `spit inputs` stays as a report.
- **Blob ids stay.** The runner reads `pipeline_files` and their git blob ids to tell when a pipeline changed (`changed_pipeline_files` in `runner/src/spit_bash/load.py`), and this branch keeps the version-18 contract. The OCaml writer emits an empty `pipeline_files` today, which is a parity gap.
- **`check --path-rules` is not yet decided.** The Rust compiler listed each product's path rule and where its extension came from; the tests that wait on whether that listing stays say so.
- **Both policy checks are recreated for OCaml:** modules use only their layer and below, files stay short, and OCaml sources are comment-free, as they already are. AGENTS.md states the comment rule for Rust; it gains the OCaml rule when the agent notes are rewritten for the OCaml tree.

Shared fixtures that are already language-neutral carry over unchanged: `tests/fixtures/comments.txt` (comment stripping), `tests/fixtures/weekly_stores`, `tests/fixtures/stages`, and the stored outputs in `tests/fixtures/outputs` other than `complete_spitout.txt` and `reviews_spitout.txt`, which are regenerated in the new syntax and compared as goldens.

## Syntax and layout (130)

### `blocks.rs`: port

- [ ] a_keyword_block_reads_as_its_statements_written_one_per_line
- [ ] an_operation_block_may_hold_an_operation_carried_out_by_steps
- [ ] an_entry_fails_as_its_keywords_statement_would
- [ ] the_entries_of_a_block_share_one_indentation
- [ ] only_a_command_goes_on_over_indented_lines
- [ ] a_step_goes_on_while_its_brackets_are_open
- [ ] a_block_with_no_entries_is_no_error
- [ ] a_recipe_writes_its_rules_and_source_paths_in_blocks
- [ ] the_editor_knows_a_block_entry_and_a_continued_line_by_their_keyword `drop` (the editor contract is dropped; #423)

### `clause_spelling.rs`: drop (the clause syntax changed)

- [ ] a_space_after_the_at_of_any_clause_is_rejected_with_its_replacement
- [ ] the_compact_spelling_is_accepted_for_every_clause
- [ ] an_unknown_word_after_a_spaced_at_gets_the_error_that_lists_the_clauses

### `collections.rs`: port

- [ ] a_join_and_a_list_keep_their_artifacts_in_natural_order
- [ ] a_separator_may_hold_quotes_parentheses_and_spaces
- [ ] each_mistake_says_what_is_wrong
- [ ] quoted_it_is_text

### `columns.rs`: port

- [ ] syntax_errors_point_at_the_offending_token
- [ ] syntax_errors_without_a_token_point_at_the_line_content
- [ ] step_errors_point_at_the_part_of_the_step_at_fault
- [ ] template_errors_point_at_the_placeholder_or_template
- [ ] warnings_point_at_the_name_or_word
- [ ] rule_and_inventory_errors_point_at_the_rule_and_record
- [ ] unclosed_brackets_point_from_the_opener_and_missing_arrows_at_the_type
- [ ] unclosed_generic_brackets_point_at_the_specific_opener
- [ ] invalid_type_variables_point_at_the_variable_not_the_whole_type
- [ ] recipe_rule_errors_point_at_the_product_or_the_grouped_dimensions
- [ ] errors_in_records_written_in_a_recipe_point_at_the_record
- [ ] command_errors_about_the_operation_point_at_its_name
- [ ] a_step_output_does_not_repeat_its_inputs_dimension_error
- [ ] display_in_counts_the_column_in_characters `drop` (tests a Rust Display or helper)
- [ ] steps_point_at_their_parts
- [ ] lines_are_found_as_str_lines_finds_them `drop` (tests a Rust Display or helper)

### `command_sequences.rs`: port

- [ ] a_job_runs_each_line_as_a_command_of_its_own
- [ ] a_block_of_one_line_is_the_same_job_as_the_line_written_after_the_colon
- [ ] a_line_indented_further_goes_on_with_the_line_above
- [ ] a_command_block_entry_ending_in_a_colon_holds_lines
- [ ] every_placeholder_works_on_every_line
- [ ] the_plan_shows_a_run_line_for_each_command
- [ ] every_output_is_used_by_some_line
- [ ] one_line_at_most_writes_an_output_as_its_stdout
- [ ] a_stdout_output_is_read_by_the_lines_after_it_only
- [ ] an_error_in_a_line_points_at_that_line
- [ ] each_line_is_indented_as_the_first
- [ ] verify_and_check_hold_one_command_each
- [ ] a_command_with_no_lines_is_empty

### `multiline_operations.rs`: port

- [ ] multiline_lists_match_single_line_signatures
- [ ] stages_and_composite_bodies_start_after_the_complete_signature
- [ ] malformed_port_points_at_its_physical_line
- [ ] unclosed_signature_does_not_consume_later_declarations

### `parser.rs`: port

- [ ] dimensionless_sources_accept_bare_names_and_match_each_job
- [ ] parses_a_pipeline_its_recipe_and_its_records
- [ ] rules_and_records_belong_outside_the_pipeline
- [ ] reports_line_for_bad_text
- [ ] rejects_duplicate_dimension_in_source
- [ ] rejects_source_inventory_inside_pipeline_file
- [ ] separate_pipeline_still_parses_without_inventory
- [ ] flow_form_infers_intermediate_products_and_keeps_inventory_separate
- [ ] flow_form_requires_operation_declaration_before_use
- [ ] command_arguments_keep_quoted_hashes_and_strip_comments
- [ ] equals_command_keeps_colons_in_arguments
- [ ] shell_source_says_to_put_the_program_on_path
- [ ] rejects_unbalanced_command_brackets_with_line_number
- [ ] path_template_errors_are_reported_while_parsing
- [ ] hash_inside_a_word_is_text_as_in_bash
- [ ] input_port_cannot_shadow_output_placeholder
- [ ] named_output_cannot_use_the_builtin_placeholder_name
- [ ] removed_sections_say_what_to_write_instead `drop` (migration diagnostic for an old spelling)
- [ ] a_source_record_may_name_its_file
- [ ] an_empty_named_contexts_section_records_its_discovery
- [ ] nested_sources_expand_each_product_and_run
- [ ] every_input_port_has_a_name
- [ ] a_command_names_each_port_it_uses

### `src/parser/check.rs`: merge

- [ ] a_check_declares_its_parameters_and_command
- [ ] a_default_list_separates_the_checks_it_drops
- [ ] clauses_split_after_the_port_and_name_their_checks

### `src/parser/flow.rs`: merge

- [ ] a_line_that_cannot_start_a_step_names_its_first_word

### `src/parser/keyword.rs`: merge

- [ ] lines_are_classified_by_their_first_word

### `src/parser/layout.rs`: merge

- [ ] each_line_is_read_once_with_its_head
- [ ] heads_follow_the_keyword_and_header_rules
- [ ] records_are_marked_and_blank_to_statements
- [ ] an_operation_signature_spans_lines_while_its_parentheses_are_open
- [ ] blocks_come_from_indentation_and_the_opening_line
- [ ] keyword_blocks_and_statements_over_several_lines
- [ ] a_step_is_names_then_a_call

### `src/parser/resources.rs`: merge

- [ ] cores_and_memory_are_read_with_their_units
- [ ] bad_values_say_what_to_write

### `src/parser/review.rs`: merge

- [ ] a_review_line_declares_a_review_or_names_its_file
- [ ] a_review_takes_no_command_and_no_parameters
- [ ] a_default_list_separates_the_reviews_it_drops
- [ ] clauses_attach_checks_and_reviews_together

### `src/parser/scan.rs`: merge

- [ ] a_bracket_has_the_depth_of_the_text_around_it
- [ ] a_closer_matches_only_an_opener_of_its_kind
- [ ] unclosed_gives_the_earliest_opener_of_the_first_kind
- [ ] only_the_kinds_asked_for_nest
- [ ] crossed_brackets_leave_each_kind_open_on_its_own
- [ ] closing_paren_skips_nested_pairs

### `src/parser/tools.rs`: merge

- [ ] a_uses_line_names_an_operation_and_its_tools
- [ ] bad_lists_are_errors

### `src/span.rs`: merge

- [ ] a_slice_is_found_in_the_line_it_came_from
- [ ] an_address_outside_the_line_or_inside_a_character_is_rejected
- [ ] content_leaves_out_indentation_trailing_space_and_comments
- [ ] comments_are_stripped_as_the_shared_fixture_says

### `syntax_errors.rs`: port

- [ ] reports_syntax_line_from_unsaved_text
- [ ] reports_independent_syntax_errors_across_a_pipeline
- [ ] a_line_that_fails_still_closes_the_stage_it_is_outdented_from
- [ ] reports_pipeline_and_inventory_syntax_errors_together
- [ ] seeded_deletions_report_every_damaged_pipeline_line
- [ ] seeded_deletions_report_every_damaged_inventory_line
- [ ] deletion_messages_name_the_missing_syntax_without_cascading
- [ ] records_in_a_pipeline_are_one_error_however_many_lines
- [ ] a_line_that_fails_still_ends_the_body_before_it
- [ ] calls_to_an_operation_that_failed_to_declare_repeat_its_error
- [ ] each_call_to_a_misspelled_operation_is_an_error_of_its_own
- [ ] each_bad_step_of_a_body_is_an_error_and_the_operation_keeps_the_rest
- [ ] a_body_with_every_step_failing_is_reported_by_its_steps_alone
- [ ] a_repeated_path_extension_or_check_list_is_an_error_of_its_own
- [ ] the_first_line_of_a_stage_sets_its_indentation_even_when_it_fails
- [ ] a_line_that_fails_still_closes_the_stage_before_it
- [ ] a_failed_line_that_closes_a_stage_keeps_it_closed_on_every_pass
- [ ] each_call_whose_selectors_clash_with_its_body_is_an_error_of_its_own
- [ ] a_stage_header_that_fails_keeps_its_lines
- [ ] the_steps_of_a_body_whose_header_fails_are_read_as_ordinary_statements
- [ ] a_default_without_its_colon_is_an_error
- [ ] an_equals_inside_a_call_is_not_the_steps_assignment
- [ ] a_check_parameter_with_nested_parentheses_is_a_bad_parameter
- [ ] an_unclosed_parenthesis_before_the_equals_is_reported_as_unclosed
- [ ] a_bad_input_name_is_reported_before_a_bad_clause

### `type_errors.rs`: port

- [ ] a_multi_byte_character_in_a_type_is_an_error_with_a_place
- [ ] a_non_ascii_character_inside_angle_brackets_is_reported_at_it
- [ ] check_json_reports_the_same_location_and_fails `drop` (the editor contract is dropped; #423)

## Diagnostics (57)

### `diagnostics.rs`: port

- [ ] validates_external_inventory_and_semantics
- [ ] checked_diagnosis_retains_the_original_pipeline_and_inventory `drop` (tests a Rust library boundary, not behaviour)
- [ ] type_errors_point_to_the_exact_flow_step_even_when_operation_is_reused
- [ ] inferred_type_error_points_to_the_consuming_step
- [ ] source_inventory_errors_point_to_the_source_line
- [ ] duplicate_declaration_points_to_the_second_declaration
- [ ] a_product_two_steps_make_quiets_every_step_that_reads_it
- [ ] missing_join_input_points_to_the_call
- [ ] a_coverage_error_names_the_failing_rule_when_rules_share_a_product
- [ ] every_semantic_error_is_reported_once_in_line_order
- [ ] uses_of_a_failed_declaration_or_step_are_not_reported_again
- [ ] undeclared_operation_is_reported_when_its_name_prefixes_an_invalid_one
- [ ] source_with_wrong_dimensions_points_to_its_inventory_line
- [ ] hash_ending_a_word_is_flagged_as_a_likely_comment
- [ ] path_rule_errors_point_to_the_rule_in_use
- [ ] a_cycle_is_reported_once_at_the_step_it_was_found_at
- [ ] every_independent_cycle_is_reported_once_and_quiets_what_reads_it
- [ ] independent_errors_on_one_line_are_each_reported
- [ ] a_misspelled_dimension_is_not_also_reported_as_omitted
- [ ] unrelated_errors_at_one_span_are_each_reported

### `imported_errors.rs`: port

- [ ] a_body_that_reads_a_missing_product_is_reported_in_the_library
- [ ] an_undeclared_operation_is_reported_in_the_library
- [ ] a_bad_placeholder_is_reported_in_the_library
- [ ] a_library_that_imports_a_broken_library_names_each_use_line
- [ ] a_library_error_does_not_hide_an_error_on_the_same_line_of_the_pipeline
- [ ] a_recipe_names_its_pipelines_library_relative_to_the_recipe
- [ ] a_failed_import_does_not_hide_an_error_in_another_library

### `json.rs`: drop (`check --json` is dropped)

- [ ] cli_accepts_stdin_and_returns_json `drop` (the editor contract is dropped; #423)
- [ ] cli_reports_semantic_error_line_in_json `drop` (the editor contract is dropped; #423)
- [ ] cli_json_includes_each_severity_and_its_columns `drop` (the editor contract is dropped; #423)
- [ ] cli_json_columns_count_utf16_code_units `drop` (the editor contract is dropped; #423)
- [ ] recipe_json_places_pipeline_errors_in_the_pipeline_file `drop` (the editor contract is dropped; #423)
- [ ] a_code_names_the_variant_of_the_error

### `severity.rs`: drop (it compares text with `check --json`, which is dropped)

- [ ] check_and_its_json_give_each_diagnostic_one_severity `drop` (the editor contract is dropped; #423)
- [ ] every_example_checks_the_same_both_ways

### `src/diagnostics/diagnostic.rs`: drop (diagnostic codes were for `check --json`)

- [ ] a_code_is_the_variant_names_in_lower_case

### `src/parser/error/body.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/check.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/command.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/declarations.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/entities.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/flow.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/inventory.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/lexical.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/lower.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/operation.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/order.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/recipe.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/resources.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/review.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/rules.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `src/parser/error/tools.rs`: drop

- [ ] each_variant_words_the_text_it_holds `drop` (tests a Rust Display or helper)

### `warnings.rs`: port

- [ ] warnings_flag_unused_definitions_missing_commands_and_unbound_type_variables
- [ ] a_line_with_an_error_shows_no_warnings
- [ ] steps_that_resolve_no_jobs_are_reported
- [ ] steps_that_read_a_product_twice_are_checked_in_linear_time `bench` (timing claim: belongs in profiling)
- [ ] a_product_named_after_its_operation_is_warned_about

## Types (54)

### `declared_types.rs`: port

- [ ] a_declaration_names_the_parameters_in_order
- [ ] types_written_with_the_declared_arguments_pass
- [ ] a_source_with_too_few_or_too_many_arguments_is_reported_where_it_is_written
- [ ] a_nested_type_is_checked_in_turn
- [ ] an_operation_port_with_the_wrong_arguments_is_reported_at_the_operation_and_quiets_its_steps
- [ ] a_type_that_is_not_declared_is_not_checked_unless_strict
- [ ] a_bad_declaration_is_reported
- [ ] an_import_brings_its_declarations_and_conflicts_on_different_ones

### `stress_pipelines.rs`: port

- [ ] nested_types_propagate_across_polymorphic_branches_and_rollups
- [ ] observatory_resolves_each_scope_and_reuses_generic_evidence
- [ ] wrong_modality_calibration_fails_after_inferred_type_flows_downstream
- [ ] nested_pair_rejects_a_second_lidar_stream
- [ ] declared_nested_output_cannot_override_inferred_reference_space
- [ ] missing_peer_at_one_slice_does_not_cross_join_another_capture
- [ ] inferred_intermediate_type_mismatch_fails_even_with_no_artifacts

### `strict_types.rs`: port

- [ ] a_type_declared_once_and_inferred_through_generic_steps_passes
- [ ] without_the_line_or_with_types_standard_missing_types_are_not_errors
- [ ] an_untyped_source_is_reported_at_its_declaration_and_quiets_what_reads_it
- [ ] a_source_with_an_unknown_part_is_reported
- [ ] an_untyped_port_of_a_used_operation_is_reported_once_and_quiets_its_steps
- [ ] a_variable_no_input_fixes_is_reported_at_the_product_and_a_written_type_fixes_it
- [ ] a_nested_variable_left_open_in_one_output_of_several_is_reported_at_that_output
- [ ] a_call_of_an_operation_with_a_body_is_held_to_typed_ports
- [ ] the_importing_file_decides_and_its_setting_covers_imported_definitions
- [ ] a_misplaced_or_misspelled_types_line_is_an_error
- [ ] a_variable_left_open_in_a_body_is_reported_at_the_call_with_the_body_s_name
- [ ] a_recipe_s_types_line_belongs_in_the_pipeline

### `types.rs`: port

- [ ] exact_named_types_match
- [ ] different_named_types_fail
- [ ] different_constructors_and_arities_fail
- [ ] generic_input_infers_variable_and_substitutes_output
- [ ] generic_import_preserves_image_space
- [ ] multiple_variables_infer_independently
- [ ] conflicting_variable_reports_previous_and_required_types
- [ ] nested_types_unify_recursively
- [ ] explicit_long_type_variables_are_distinct_from_named_types
- [ ] explicit_type_variables_are_rejected_in_product_types
- [ ] later_known_input_refines_an_earlier_partial_variable_binding
- [ ] partial_type_cannot_hide_a_downstream_known_conflict
- [ ] unknown_is_indeterminate_without_binding_a_variable
- [ ] unresolved_output_variables_become_unknown_at_job_boundary
- [ ] generic_pipeline_infers_output_type_without_pipeline_annotations
- [ ] chained_generic_operations_propagate_concrete_type
- [ ] operation_type_variables_do_not_leak_between_invocations
- [ ] field_survey_reuses_image_operations_across_kinds_and_spaces
- [ ] analytics_join_key_variables_reject_mismatched_relations
- [ ] conflicting_port_bindings_are_a_structured_resolver_error
- [ ] declared_output_type_cannot_contradict_inferred_type
- [ ] resolves_untyped_pipeline_by_shape_and_cardinality
- [ ] partially_typed_pipeline_accepts_unknown_and_rejects_known_mismatch
- [ ] a_type_variable_prints_marked_so_it_differs_from_a_named_type
- [ ] calls_of_one_generic_body_infer_their_types_apart
- [ ] a_type_a_call_of_a_generic_body_cannot_have_is_reported_at_that_call
- [ ] a_call_of_an_operation_with_a_body_is_held_to_its_signature

## Matching and resolution (60)

### `command_dimensions.rs`: port

- [ ] a_command_and_its_verify_are_given_the_jobs_values_as_text
- [ ] a_value_joins_the_commands_as_the_same_text_written_out_would
- [ ] an_aggregate_gives_the_dimensions_it_keeps
- [ ] a_dimension_the_jobs_lack_is_an_error_at_the_placeholder
- [ ] every_step_calling_the_operation_must_have_the_dimension
- [ ] an_operation_no_step_calls_is_checked_when_one_does
- [ ] a_port_comes_before_a_dimension_of_the_same_name
- [ ] a_dimension_is_text_not_a_file
- [ ] a_call_gives_the_steps_it_makes_the_callers_dimensions
- [ ] a_many_input_gives_each_artifacts_value_in_the_order_of_its_paths
- [ ] a_one_input_gives_its_artifacts_value_within_an_argument
- [ ] an_input_dimension_the_product_lacks_is_an_error_at_the_placeholder

### `commands.rs`: port

- [ ] commands_are_validated_even_without_resolved_jobs
- [ ] unnamed_output_placeholder_does_not_name_a_multi_output_port

### `matching.rs`: port

- [x] a_many_input_can_share_an_operation_with_single_inputs
- [x] one_aggregate_can_vary_two_dimensions_in_product_order
- [x] a_call_varies_one_or_several_dimensions
- [x] a_single_input_of_an_aggregate_must_match_each_group
- [x] a_missing_broadcast_suggests_the_input_and_dimension
- [x] an_operation_can_collect_two_products_across_the_same_dimensions
- [x] collections_that_hold_different_artifacts_are_not_paired
- [x] collections_must_vary_over_the_same_dimensions
- [x] where_pins_a_dimension_the_driver_lacks
- [x] where_filters_the_driver_and_removes_its_dimension_from_the_output
- [x] selectors_combine_on_one_binding
- [x] same_matches_on_fewer_dimensions_and_requires_one_artifact
- [x] selectors_are_checked_against_the_port_and_product
- [x] each_runs_a_step_for_every_value_an_input_broadcasts
- [x] a_broadcast_dimension_can_be_collected_again
- [x] broadcasts_are_checked_against_the_step
- [x] port_order_does_not_decide_the_driving_input
- [x] collections_and_jobs_follow_natural_declared_order
- [x] first_and_last_choose_from_each_ragged_group_after_filtering
- [x] adding_a_later_artifact_leaves_the_dag_byte_for_byte_unchanged
- [x] check_validates_selection_dimensions_and_provides_hovers (the selector errors are ported; the hovers are dropped)
- [x] a_body_can_select_from_its_callers_filtered_input
- [x] min_rejects_a_collection_that_is_too_small

### `multiple_outputs.rs`: port

- [ ] one_job_owns_every_output_of_an_operation
- [ ] outputs_and_verifications_are_checked_before_resolution

### `order.rs`: port

- [ ] a_pair_no_source_orders_must_be_declared
- [ ] every_product_follows_the_declared_order
- [ ] sources_must_agree_with_each_other_and_with_the_line
- [ ] the_line_names_every_dimension_once_and_no_other
- [ ] a_written_order_is_a_check_not_an_override
- [ ] no_product_may_be_named_dimensions `merge` (one reserved-names case)

### `resolver.rs`: port

- [x] expands_one_to_one_over_two_runs
- [x] reuses_less_specific_t1_across_runs
- [x] reports_missing_input
- [x] rejects_secondary_input_with_dimensions_absent_from_driver
- [x] checks_types_before_concrete_expansion
- [x] aggregates_each_fixed_dimension_group
- [x] rejects_accidental_cartesian_product
- [x] tracks_dependencies_through_full_pipeline
- [ ] catches_duplicate_source_artifact `merge` (a dataset on disk gives an identity twice only through stars; `messy_sources` covers it)
- [x] catches_product_cycle
- [x] source_inventory_changes_job_count_without_changing_pipeline
- [x] named_ports_and_the_calls_aggregate_shape_are_checked
- [x] a_many_input_names_its_dimensions_at_the_call
- [x] pipeline_checks_need_no_inventory (its `command_demo` example waits for the examples in the new syntax)

### `undeclared_files.rs`: port

- [ ] a_file_a_command_runs_without_declaring_it_is_named

## Composites, imports and stages (67)

### `composite_examples.rs`: merge

- [ ] every_job_of_a_call_can_be_inspected
- [ ] a_calls_steps_are_listed_without_reading_data
- [ ] a_call_makes_the_jobs_the_steps_it_replaces_made
- [ ] an_error_leads_back_to_the_call_and_the_line_of_the_body
- [ ] two_calls_share_no_file
- [ ] checks_add_up_on_the_step_that_makes_an_output
- [ ] an_import_alone_adds_no_jobs

### `composites.rs`: port

- [ ] a_call_becomes_its_bodys_steps_over_the_callers_products
- [ ] call_input_attachments_reach_nested_readers_and_stay_local_to_one_call
- [ ] two_calls_file_their_own_products_apart
- [ ] a_call_in_a_body_is_expanded_in_turn_and_in_the_callers_stage
- [ ] a_body_is_checked_where_it_is_declared
- [ ] a_call_is_checked_at_the_call
- [ ] a_declared_output_type_holds_the_product_the_body_makes
- [ ] an_operation_with_a_body_needs_no_command_and_its_call_uses_it
- [ ] an_imported_operation_brings_the_operations_its_body_calls
- [ ] checks_on_a_bodys_ports_and_outputs_run_on_the_steps_that_read_and_make_them
- [ ] the_spitdag_names_each_file_read_each_call_and_where_each_job_comes_from
- [ ] counts_list_a_calls_steps_under_it_and_commands_say_where_a_job_comes_from
- [ ] an_error_in_a_call_points_at_the_argument_the_caller_gave_and_into_the_body
- [ ] an_error_in_an_imported_nested_call_relates_each_call_and_the_step
- [ ] what_cannot_be_made_names_the_call_it_comes_from
- [ ] hovers_on_a_call_describe_the_call_and_the_steps_it_expands_to `drop` (the editor contract is dropped; #423)
- [ ] a_call_is_not_named_as_an_import_is
- [ ] calls_list_each_written_call_with_its_steps_and_the_calls_in_them
- [ ] a_step_written_in_the_pipeline_says_which_line_of_which_file_it_is

### `imports.rs`: port

- [ ] unqualified_and_nested_imports_work_together
- [ ] import_errors_point_to_the_use_line
- [ ] messages_name_a_library_from_the_pipelines_folder
- [ ] diagnostics_resolve_imports_using_pipeline_location
- [ ] an_operation_a_library_declares_in_a_stage_may_be_used_anywhere
- [ ] a_local_operation_may_not_take_an_imported_name
- [ ] an_imported_source_keeps_its_path_rule_under_its_alias
- [ ] quoted_import_path_can_contain_as
- [ ] import_all_brings_definitions_but_not_steps
- [ ] imported_definitions_are_not_reported_as_unused
- [ ] an_imported_path_keeps_escaped_braces
- [ ] an_import_may_not_define_again_what_the_file_defines
- [ ] an_imported_check_conflicts_only_when_it_differs
- [ ] an_import_brings_a_sidecars_group_in_whole
- [ ] a_sidecars_member_is_not_imported_alone
- [ ] an_imported_sidecars_group_still_reports_incomplete_bindings

### `stages.rs`: port

- [ ] steps_belong_to_the_stage_whose_block_holds_them
- [ ] an_unindented_line_ends_a_stage
- [ ] no_product_may_be_named_stage `merge` (one reserved-names case)
- [ ] jobs_carry_their_stage
- [ ] a_stage_path_rule_covers_only_that_stage
- [ ] stage_placeholder_needs_a_stage
- [ ] stages_may_read_from_each_other_in_a_cycle
- [ ] stages_that_only_read_forward_are_accepted
- [ ] stage_syntax_errors
- [ ] a_stage_opened_again_continues_its_first_block
- [ ] an_empty_stage_is_reported
- [ ] check_counts_jobs_per_stage
- [ ] nested_stages_are_named_by_their_path
- [ ] the_same_name_can_be_nested_in_different_stages
- [ ] nested_stages_nest_their_paths_and_inherit_defaults
- [ ] an_outer_stage_counts_the_jobs_of_the_stages_nested_in_it
- [ ] nested_stages_may_read_from_each_other_in_a_cycle
- [ ] a_nested_stage_may_read_from_its_outer_stage_and_siblings
- [ ] a_stage_whose_steps_are_all_nested_is_not_empty
- [ ] an_operation_called_outside_its_stage_says_where_to_declare_it
- [ ] the_stage_to_declare_in_holds_every_outside_call
- [ ] an_operation_called_within_its_stage_is_in_place
- [ ] a_library_may_group_operations_in_stages
- [ ] verified_files_name_what_was_checked
- [ ] every_job_follows_the_jobs_it_depends_on

## Path rules, shapes and extensions (93)

### `built_in_placeholders.rs`: port

- [x] a_dimension_may_take_a_built_in_name
- [ ] an_old_form_says_what_to_write `drop` (migration diagnostic for an old spelling)
- [x] an_unknown_built_in_is_an_error
- [x] optional_groups_and_labels_resolve_for_each_cohort_product
- [x] optional_groups_reject_unknown_dimensions_and_invalid_nesting
- [x] a_dimensionless_product_omits_an_optional_entities_group
- [ ] labels_are_gone_and_say_what_to_write_instead `drop` (migration diagnostic for an old spelling)
- [ ] an_explicit_optional_path_has_an_editor_hint `drop` (the editor contract is dropped; #423)
- [x] discovery_uses_each_sources_resolved_groups_and_entities

### `entities_format.rs`: port

- [x] one_format_covers_each_shape_without_changing_identity
- [x] a_dimensionless_product_drops_an_optional_group_and_output_rules_use_the_format
- [ ] fresh_discovery_and_saved_inventory_produce_the_same_dag_and_hints `drop` (#418 removes the `.spitout`)
- [ ] imports_keep_library_source_formats_and_use_caller_output_formats (not yet ported)
- [x] aliases_can_precede_the_format_but_need_one_and_no_product_is_named_entities
- [x] discovery_decodes_values_and_escaped_aliases_and_accepts_assignment_affixes
- [ ] bad_declarations_are_located_and_old_forms_say_what_to_write `drop` (migration diagnostic for an old spelling)
- [x] a_stage_spelling_covers_its_products_and_the_stages_nested_in_it
- [ ] a_recipe_spelling_spells_the_recipes_own_source_rules (gap: `default entities` in a recipe)
- [x] a_default_that_needs_entities_leaves_a_dimensionless_source_to_its_own_rule

### `extensions.rs`: port

- [x] an_operation_completes_the_default_path_with_its_outputs_extensions
- [x] ext_completes_the_default_for_operations_that_declare_none
- [x] a_stage_ext_applies_to_its_steps_and_nested_stages
- [x] a_products_own_rule_takes_only_its_operations_extension
- [x] a_source_on_the_default_rule_takes_ext
- [x] a_source_declares_its_extension_as_an_output_does
- [x] a_sources_rule_ending_in_another_extension_is_an_error
- [x] bound_paths_carry_the_extension
- [x] a_rule_ending_in_another_extension_is_an_error
- [x] a_dot_earlier_in_the_file_name_is_not_part_of_its_extension
- [x] a_default_rule_ending_in_another_extension_is_one_error
- [x] extensions_are_checked_when_parsed
- [x] a_recipe_cannot_set_ext
- [x] an_imported_operation_brings_its_extension
- [ ] check_says_where_each_extension_comes_from (waits on a decision about `check --path-rules`)

### `path_roundtrip.rs`: port

- [ ] a_rendered_path_matches_back_to_its_binding

### `paths.rs`: port

- [ ] path_coverage_exposes_default_fallbacks_and_accepts_them (waits on a decision about `check --path-rules`)
- [x] an_output_with_no_rule_is_an_error_and_a_source_waits_for_one
- [x] path_coverage_catches_missing_and_invalid_rules_without_jobs
- [ ] bound_dag_shows_port_names_and_paths_without_commands (its example waits for the examples in the new syntax)
- [x] path_rules_that_cannot_separate_artifacts_are_rejected
- [x] a_default_naming_a_dimension_a_source_lacks_leaves_it_to_the_recipe

### `placeholder_shapes.rs`: port

- [x] a_rule_without_a_shape_reads_every_name_as_a_date
- [x] a_date_shape_reads_only_real_dates
- [ ] a_file_failing_the_shape_is_counted_as_unmatched (gap: the unmatched-file report waits for #418)
- [ ] the_nearest_file_shows_the_rule_with_its_shape (gap: nearest-file hints come with `missed_sources`)
- [x] adjacent_placeholders_split_where_a_fixed_shape_ends
- [x] a_year_then_digits_need_no_separator
- [x] a_shape_applies_to_part_of_a_name
- [x] a_dimension_written_twice_keeps_its_shape_in_both_places
- [x] a_leading_zero_is_kept
- [x] two_rules_that_both_read_a_file_still_conflict
- [ ] a_recipe_rule_and_a_discover_rule_take_shapes (the recipe rule is ported; `contexts:` is a gap)
- [x] a_bad_shape_says_which_shapes_there_are
- [x] a_built_in_placeholder_takes_no_shape
- [x] one_dimension_has_one_shape
- [x] two_open_shapes_need_text_between
- [x] a_shape_on_an_output_or_a_default_is_an_error
- [ ] a_record_whose_value_fails_its_rules_shape_is_rejected `drop` (#418 removes the `.spitout` its record came from)
- [x] two_sources_with_one_dimension_name_collide_whatever_their_shapes
- [x] the_default_owner_error_names_the_rule_to_write
- [ ] a_dropped_group_can_leave_two_open_shapes_touching (ported for a source rule; the recipe `default:` form is a gap)
- [x] a_dropped_group_leaving_a_separator_is_fine
- [x] a_source_placeholder_of_any_length_beside_another_is_an_error
- [x] a_fixed_length_shape_beside_an_unshaped_placeholder_splits_where_it_ends
- [x] an_output_may_put_placeholders_side_by_side
- [x] a_date_on_a_leap_day_follows_the_century_rule
- [ ] a_folder_source_takes_a_shape (gap: folder sources)
- [ ] a_beside_companion_inherits_the_shape (gap: beside sources)
- [x] a_shape_inside_an_optional_group_applies_where_the_group_is_kept
- [ ] a_recipes_shared_default_takes_a_shape (gap: recipe `default:`)

### `src/paths/components.rs`: merge

- [ ] unusable_paths_are_named_by_their_first_problem

### `src/template/collection.rs`: merge

- [ ] a_join_has_a_separator_and_a_list_none
- [ ] mistakes_say_what_to_write

### `src/template/command.rs`: merge

- [ ] named_files_are_relative_paths_as_text
- [ ] a_decimal_is_a_finite_number_in_digits
- [ ] a_plain_word_is_left_as_it_is
- [ ] any_other_word_is_quoted
- [ ] quoted_words_split_back_into_the_same_words

### `src/template/glob.rs`: merge

- [ ] a_folder_covers_what_is_under_it
- [ ] a_star_matches_within_one_name
- [ ] two_stars_are_a_literal_star
- [ ] placeholders_and_empty_paths_are_rejected

### `src/template/pattern.rs`: merge

- [ ] components_keep_letters_digits_and_dashes_and_encode_the_rest
- [ ] every_shape_accepts_only_value_characters
- [ ] a_rendered_path_matches_back_with_the_shape_written_once
- [ ] a_star_matches_any_text_within_one_name
- [ ] a_doubled_star_is_a_literal_star
- [ ] a_fixed_value_matches_no_text_and_is_read_with_the_rest
- [ ] stars_that_cannot_match_fail_quickly

### `src/template/shape.rs`: merge

- [x] a_fixed_length_is_the_length_of_every_value
- [x] the_century_rule_for_leap_days
- [x] a_date_must_be_a_real_day
- [x] years_and_digits

## Placement: beside, sidecars, folders, roots (88)

### `beside.rs`: port (as path rules with `{name$dir}` and `{name$stem}`; `beside` is removed, D13 in `ocaml/language-plan.md`)

- [ ] a_beside_output_takes_its_siblings_path_and_name
- [ ] a_beside_output_follows_its_siblings_own_rule
- [ ] the_command_may_leave_a_beside_output_out_and_the_plan_keeps_it
- [ ] a_beside_output_has_no_path_rule_of_its_own
- [ ] a_beside_output_names_a_sibling_with_an_extension
- [ ] a_beside_output_cannot_share_a_path_with_another_product
- [ ] the_path_rule_listing_says_where_a_beside_output_gets_its_path
- [ ] an_output_cannot_be_written_beside_itself

### `beside_inputs.rs`: merge (an output beside an input is a path rule over the input, and stays under the output root)

- [ ] an_output_beside_a_source_lies_beside_its_file_and_its_reader_waits_for_it
- [ ] an_output_beside_a_source_follows_the_rule_a_recipe_gives_it
- [ ] an_output_beside_an_input_takes_the_value_the_step_pins
- [ ] a_plain_suffix_beside_an_input_replaces_its_extension
- [ ] an_output_beside_an_output_of_another_step_lies_in_its_folder
- [ ] a_plus_suffix_follows_the_whole_name_in_each_place_beside_is_written
- [ ] the_input_an_output_is_written_beside_gives_each_job_its_own_file
- [ ] an_output_lies_only_beside_a_file_with_the_extension_it_replaces
- [ ] an_output_beside_a_source_is_neither_a_source_nor_an_unmatched_file
- [ ] an_output_cannot_be_written_inside_a_source_folder

### `dataset_root.rs`: port

- [ ] a_root_line_says_where_the_data_is
- [ ] a_recipe_must_name_its_root
- [ ] a_pipeline_runs_without_a_recipe_given_its_data_folder
- [ ] the_root_line_is_relative_to_the_recipe_or_absolute
- [ ] a_root_line_does_not_make_written_records_a_scan
- [ ] a_spitout_records_its_root_relative_to_itself `drop` (#418 removes the `.spitout`)
- [ ] a_printed_spitout_records_its_root_absolute `drop` (#418 removes the `.spitout`)
- [ ] an_inventory_binds_the_same_whether_written_piped_or_redirected `drop` (#418 removes the `.spitout`)
- [ ] a_spitout_names_its_root_once_and_first `drop` (#418 removes the `.spitout`)
- [ ] a_recipe_names_its_root_once
- [ ] check_warns_when_the_root_is_missing
- [ ] a_recipe_inside_its_root_is_not_an_unmatched_file

### `folder_and_stem.rs`: port

- [ ] a_command_gives_an_outputs_folder_and_name
- [ ] the_plan_names_the_file_each_folder_and_name_is_of
- [ ] a_folder_or_name_counts_as_writing_the_output
- [ ] folders_and_names_are_checked

### `folders.rs`: port

- [x] a_folder_source_is_found_and_its_files_are_not_unmatched (found and planned; the unmatched-file part waits for #418)
- [x] a_folder_is_given_to_a_command_by_its_path
- [x] the_plan_marks_each_folder
- [ ] hovers_and_path_rules_show_a_folder `drop` (the editor contract is dropped; #423)
- [x] a_folder_may_have_an_extension_and_takes_no_default_one (takes no default extension; a folder with an extension is a syntax error in the agreed syntax)
- [ ] a_path_of_the_wrong_kind_is_named
- [ ] a_folder_rule_that_matches_nothing_names_the_nearest_folder
- [x] a_missing_source_folder_is_an_error
- [ ] a_discovered_context_needs_its_folder
- [x] a_source_may_sit_in_a_source_folder
- [x] nothing_is_written_in_a_folder_or_read_from_one_a_job_writes
- [ ] a_folder_is_never_beside_another_output_nor_a_sidecar
- [x] a_seeded_folder_names_its_input_port_in_the_dag
- [x] a_seed_requires_a_single_folder_input
- [x] a_directory_view_has_members_and_no_runnable_job
- [x] directory_view_rejects_wrong_paths_and_unlisted_outputs (members outside the view are rejected)

### `here.rs`: merge

- [ ] here_is_the_folder_of_the_file_declaring_the_operation
- [ ] moving_the_pipeline_changes_no_command
- [ ] here_gives_no_dir_or_stem_and_is_no_redirection

### `here_sources.rs`: port

- [ ] a_here_source_is_an_input_under_the_pipeline_folder
- [ ] a_here_source_survives_a_spitout `drop` (#418 removes the `.spitout`)
- [ ] a_here_source_is_one_file_in_the_pipeline_folder
- [ ] a_library_outside_the_pipeline_folder_cannot_have_here_sources
- [ ] nothing_is_written_beside_a_here_source
- [ ] a_pipeline_not_read_from_a_file_has_no_here_sources

### `output_root.rs`: port

- [ ] outputs_go_under_the_output_root_and_files_beside_a_source_stay_with_it
- [ ] without_an_output_root_outputs_are_under_the_dataset_root
- [ ] commands_run_from_the_output_root_and_reach_the_dataset_from_it
- [ ] a_written_spitout_records_the_output_root_beside_the_dataset_root `drop` (#418 removes the `.spitout`)
- [ ] a_pipeline_names_its_output_root_or_takes_it_from_the_command_line
- [ ] an_output_root_is_declared_once
- [ ] output_is_a_keyword_no_product_may_take `merge` (one reserved-names case)
- [ ] a_scan_leaves_out_an_output_root_inside_the_dataset

### `prefix_outputs.rs`: port

- [ ] a_tool_is_given_the_prefix_and_each_declared_file_is_named_after_it
- [ ] each_declared_file_runs_the_checks_of_its_extension
- [ ] a_rule_ending_in_the_naming_suffix_is_kept_and_one_that_skips_it_is_an_error
- [ ] a_prefix_needs_a_naming_suffix_and_an_output

### `sidecars.rs`: port (as sources whose rules refer to another source; `tests/companion_tests.ml` covers discovery, inherited dimensions and a missing file)

- [ ] companions_inherit_dimensions_and_follow_the_anchor_path
- [ ] a_quoted_companion_suffix_is_appended_to_the_main_stem
- [ ] source_beside_requires_a_prior_file_anchor_and_its_own_suffix
- [ ] a_recipe_gives_the_anchor_path
- [ ] a_recipe_default_places_each_companion_at_the_anchor_name
- [ ] a_recipe_cannot_override_a_companion_path
- [ ] a_spitout_cannot_override_a_companion_path `drop` (#418 removes the `.spitout`)
- [ ] discovery_names_a_group_missing_a_file
- [ ] discovery_names_incomplete_groups_in_value_order
- [ ] records_name_a_group_missing_a_file
- [ ] records_count_what_an_earlier_run_removed
- [ ] a_missing_member_fails_only_the_steps_that_read_it

### `source_files.rs`: merge

- [ ] survey_compiles_when_all_required_source_files_exist
- [ ] survey_reports_a_missing_required_file
- [ ] survey_rejects_a_photo_without_an_inventory_sidecar
- [ ] survey_reports_a_source_path_that_is_a_directory
- [ ] the_spitdag_records_the_root_as_an_absolute_path

## Inputs, discovery and exclusions (134)

### `conditional_exclusions.rs`: merge

- [ ] a_conditional_exclude_rule_names_its_groups_and_the_condition_that_removes_them
- [ ] a_drop_rule_is_an_error_that_names_the_exclude_rule_to_write `drop` (migration diagnostic for an old spelling)
- [ ] a_drop_rule_in_a_stage_names_the_exclude_rule_to_write `drop` (migration diagnostic for an old spelling)
- [ ] a_retired_rule_form_gives_the_ordinary_error `drop` (migration diagnostic for an old spelling)
- [ ] a_group_with_none_of_the_target_counts_zero
- [ ] a_value_condition_is_recorded_as_a_conditional_exclusion
- [ ] rules_are_judged_together_so_their_order_does_not_matter
- [ ] a_group_two_rules_remove_is_recorded_once
- [ ] removing_every_group_is_an_error
- [ ] require_is_checked_after_conditional_exclusions_and_never_over_nothing
- [ ] a_value_clause_on_a_grouping_dimension_explains_the_rule
- [ ] a_session_missing_its_file_can_be_dropped_rather_than_fail_discovery

### `coverage.rs`: port

- [ ] coverage_rules_can_require_entity_values
- [ ] a_rule_is_checked_against_the_pipeline_without_an_inventory
- [ ] coverage_checks_each_observed_context_without_a_global_count

### `discovery.rs`: port

- [ ] unmatched_files_are_counted_and_can_be_listed_without_an_inventory
- [ ] hidden_folders_are_not_scanned_unless_a_rule_names_them
- [ ] many_unmatched_files_are_counted_by_extension
- [ ] files_at_the_pipelines_output_paths_are_not_unmatched
- [ ] sources_are_discovered_from_their_path_rules
- [ ] directory_discovery_finds_observed_subject_session_pairs
- [ ] observed_directories_require_source_files_only_with_a_rule
- [ ] directory_bindings_expand_sources_at_their_declared_dimensions
- [ ] coverage_can_target_context_dimensions
- [ ] drop_discovery_group_removes_subject_before_source_checks_and_jobs
- [ ] drop_source_group_can_omit_missing_files_in_a_discovered_context
- [ ] a_context_requirement_uses_only_its_selected_dimension_set
- [ ] context_patterns_with_equal_dimensions_union_and_deduplicate
- [ ] context_paths_reject_unsafe_or_reserved_patterns
- [ ] context_path_may_match_no_directories
- [ ] a_file_matching_two_source_rules_is_rejected
- [ ] cli_discovers_sources_under_the_root
- [ ] discovery_skips_values_spit_would_write_differently
- [ ] discovery_reports_skipped_files_and_still_succeeds
- [ ] discovery_is_fast_however_values_could_be_split `bench` (timing claim: belongs in profiling)
- [ ] discovery_follows_links_without_looping

### `exclusions.rs`: port

- [ ] an_exclude_names_an_artifact_a_group_or_part_of_a_source
- [ ] a_rule_is_checked_against_the_pipeline_without_data
- [ ] an_excluded_run_leaves_its_session_to_the_other_runs
- [ ] a_group_is_removed_from_every_source_and_recorded_once
- [ ] an_exclude_that_matches_nothing_is_an_error_naming_close_values
- [ ] records_given_directly_are_excluded_too
- [ ] a_discovered_context_left_out_needs_no_files
- [ ] excluding_an_absent_file_reports_an_unmatched_exclusion
- [ ] an_excluded_file_may_lie_outside_the_discovered_contexts
- [ ] a_failed_join_names_the_exclusion_that_caused_it
- [ ] the_record_of_what_was_removed_reads_back_and_reaches_the_spitdag

### `found_paths.rs`: port

- [ ] preferences_keep_one_path_of_each_identity_and_record_the_rest
- [ ] a_block_of_preferences_reads_as_its_lines
- [ ] a_preference_leaves_an_identity_read_once_alone
- [ ] a_star_in_an_exclusion_covers_every_copy_it_matches
- [ ] a_preference_that_leaves_two_paths_is_the_duplicate_error
- [ ] a_preference_that_covers_no_path_of_a_duplicate_is_the_duplicate_error
- [ ] a_preference_that_chooses_for_no_identity_is_an_error
- [ ] found_paths_take_text_and_stars_only
- [ ] a_preference_belongs_in_the_recipe

### `inline_inputs.rs`: port

- [ ] one_file_checks_scans_and_resolves_from_another_working_folder
- [ ] recipes_inherit_the_pipeline_root_and_add_selection_rules
- [ ] duplicate_roots_and_source_paths_are_errors_even_when_equal
- [ ] every_selection_form_is_recipe_only_including_literal_exclusions
- [ ] roots_are_top_level_once_and_require_a_directory
- [ ] imported_definitions_do_not_import_a_dataset_root
- [ ] stdin_checks_resolve_and_locate_the_inline_root
- [ ] absolute_roots_and_spaces_are_directory_text
- [ ] an_inherited_missing_root_warns_at_the_pipeline_declaration

### `input_semantics.rs`: port

- [ ] input_block_checks_its_structure_and_duplicate_paths
- [ ] matching_paths_do_not_make_optional_scene_products_mandatory
- [ ] removed_recipe_forms_are_rejected `drop` (migration diagnostic for an old spelling)
- [ ] a_literal_directory_can_establish_a_global_context
- [ ] paired_reads_require_the_missing_mate
- [ ] empty_directory_observes_a_group_and_only_require_makes_it_mandatory
- [ ] global_requirement_checks_singleton_lookup
- [ ] context_target_counts_only_its_exact_dimension_domain

### `inputs.rs`: port

- [ ] the_stage_finds_contexts_and_sources_without_touching_the_pipeline `drop` (tests a Rust library boundary, not behaviour)
- [ ] jobs_resolve_from_the_logical_pipeline_and_the_stages_inventory `drop` (tests a Rust library boundary, not behaviour)
- [ ] drop_rules_are_applied_by_the_stage_before_jobs_exist `drop` (tests a Rust library boundary, not behaviour)
- [ ] require_gaps_are_the_stages_to_report_and_do_not_reach_the_resolver `drop` (tests a Rust library boundary, not behaviour)
- [ ] a_recipe_can_settle_records_that_were_already_written
- [ ] dropping_written_records_records_the_removed_group
- [ ] records_written_in_a_recipe_are_its_inputs_and_are_not_scanned
- [ ] missing_source_coverage_names_the_path_rule_and_unmatched_file
- [ ] a_recipe_path_for_an_unknown_source_is_rejected_by_the_stage
- [ ] an_empty_inventory_is_still_a_valid_stage_result
- [ ] a_named_recipe_runs_in_memory_and_binds_output_paths `drop` (tests a Rust library boundary, not behaviour)
- [ ] explicit_spitin_uses_its_own_directory_and_require_reports_gaps
- [ ] the_stage_writes_each_sources_path_into_its_record
- [ ] a_spitout_writes_values_in_the_declared_dimension_order `drop` (#418 removes the `.spitout`)
- [ ] a_spitout_lists_sources_and_unnamed_contexts `drop` (#418 removes the `.spitout`)
- [ ] a_spitout_alone_drives_jobs_without_its_recipe `drop` (#418 removes the `.spitout`)
- [ ] a_record_cannot_redirect_one_source_away_from_its_path_rule
- [ ] a_spitout_source_path_rule_cannot_override_the_pipeline `drop` (#418 removes the `.spitout`)
- [ ] contexts_follow_the_discover_rule_and_list_in_the_order_written
- [ ] a_recipe_that_does_not_fit_its_pipeline_says_why
- [ ] a_recipe_path_is_the_default_for_sources_with_no_rule
- [ ] a_recipe_path_cannot_name_a_stage
- [ ] check_lists_a_recipe_default_as_the_recipes
- [ ] check_finds_a_bad_recipe_source_path_at_its_line
- [ ] one_recipe_default_covers_sources_of_different_extensions

### `messy_sources.rs`: port

- [ ] a_cohort_split_reads_each_identity_and_keeps_its_folder
- [ ] a_spitout_of_read_paths_plans_without_the_recipe `drop` (#418 removes the `.spitout`)
- [ ] two_folders_of_one_identity_stop_until_one_is_excluded_by_path
- [ ] every_identity_read_twice_is_named_in_one_run
- [ ] an_exclusion_by_path_that_matches_nothing_fails
- [ ] two_rules_of_one_source_may_not_match_one_file
- [ ] a_record_of_a_source_that_reads_names_its_file
- [ ] rules_that_write_take_no_star_fixed_value_or_second_rule
- [ ] a_rule_reads_or_fixes_each_dimension_once_and_only_its_own
- [ ] an_editor_is_shown_each_rule_of_a_source `drop` (the editor contract is dropped; #423)

### `missed_sources.rs`: port

- [ ] a_pipeline_rule_names_the_nearest_file_and_where_it_parts
- [ ] a_recipe_rule_names_the_nearest_file
- [ ] a_file_that_goes_on_past_the_rule_says_where_the_rule_ends
- [ ] a_rule_no_file_comes_near_names_no_file
- [ ] a_coverage_failure_points_to_the_nearest_file

### `src/inputs/discover.rs`: port

- [ ] a_directory_gone_when_it_is_read_is_absent_but_a_missing_root_is_an_error

### `src/inputs/exclusions.rs`: merge

- [ ] near_values_differ_in_case_or_leading_zeros

### `src/inputs/variants.rs`: merge

- [ ] spellings_that_differ_only_in_case_are_listed_together
- [ ] a_removed_group_still_shows_beside_its_remaining_spelling
- [ ] a_set_with_every_spelling_removed_is_left_out
- [ ] letters_outside_ascii_are_not_folded
- [ ] values_that_agree_in_case_or_differ_otherwise_are_not_variants
- [ ] values_equal_as_numbers_are_listed_together
- [ ] zeros_alone_are_equal_as_numbers
- [ ] values_with_other_characters_are_not_numbers
- [ ] values_equal_as_numbers_are_not_case_variants

### `two_files.rs`: port

- [ ] a_source_takes_its_own_rule_then_the_recipes_default_then_the_pipelines
- [ ] a_recipe_line_that_belongs_in_the_pipeline_says_so_at_its_line
- [ ] every_pipeline_only_line_in_a_recipe_is_an_error
- [ ] a_recipe_pipeline_header_in_a_pipeline_says_where_it_belongs

### `variants.rs`: port

- [ ] inputs_lists_values_that_differ_only_in_case_together
- [ ] a_group_exclusion_still_shows_the_spelling_it_missed
- [ ] a_partial_exclusion_lists_the_spelling_it_left_with_its_remaining_sources
- [ ] values_that_differ_only_outside_ascii_get_no_case_note
- [ ] a_spitout_given_directly_gets_no_case_note `drop` (#418 removes the `.spitout`)
- [ ] values_equal_as_numbers_are_listed_together

## Checks, reviews, scalars and calculations (50)

### `calculation.rs`: port

- [ ] a_calculation_emits_prefix_arrays_and_no_executable
- [ ] a_calculation_checks_its_formula_and_operation
- [ ] exponentiation_groups_right_and_binds_tighter_than_unary_minus

### `checks.rs`: port

- [ ] each_job_runs_its_checks_in_order_around_its_command
- [ ] the_spitdag_names_each_check_its_port_and_its_artifact
- [ ] checks_change_neither_the_jobs_nor_their_commands
- [ ] a_reader_runs_a_check_its_producer_skipped
- [ ] checks_and_their_uses_are_checked_where_they_are_written
- [ ] a_many_port_takes_its_minimum_and_its_checks_in_either_order
- [ ] no_product_may_be_named_check `merge` (one reserved-names case)
- [ ] imports_bring_the_checks_their_definitions_attach

### `default_checks.rs`: merge

- [ ] a_file_default_checks_every_output_but_no_input_or_source
- [ ] a_stage_default_follows_the_stage_of_the_call
- [ ] stage_defaults_add_to_the_file_and_the_stages_around_them_in_order
- [ ] a_stage_or_an_output_opts_out_with_a_bang
- [ ] an_output_opts_out_of_a_file_default_for_itself_alone
- [ ] a_check_on_one_artifact_runs_once_however_many_lists_name_it
- [ ] a_producer_default_covers_the_check_a_reader_would_repeat
- [ ] defaults_are_checked_like_other_uses
- [ ] opting_out_belongs_on_an_output
- [ ] no_product_may_be_named_check `merge` (one reserved-names case)

### `extension_checks.rs`: merge

- [ ] sources_and_outputs_run_the_checks_of_their_extension
- [ ] a_gz_file_is_not_a_nifti_and_a_folder_matches_no_extension
- [ ] a_stage_adds_to_the_file_and_an_output_opts_out
- [ ] one_list_for_an_extension_in_a_scope_and_only_declared_checks
- [ ] a_naming_suffix_with_dots_leaves_the_extension_to_select_the_check
- [ ] the_quoted_form_names_the_form_that_replaces_it `drop` (migration diagnostic for an old spelling)

### `review_snapshot.rs`: merge

- [x] each_potential_review_target_is_present_regardless_of_its_verdict
- [x] a_named_missing_file_is_distinct_from_a_review_with_no_file
- [x] a_review_on_an_input_port_names_it_and_a_source_gives_its_path
- [ ] a_dag_made_from_a_spitout_records_the_same_reviews `drop` (#418 removes the `.spitout`)

### `reviews.rs`: port

- [ ] a_review_takes_no_command_and_no_parameters
- [x] reviews_are_declared_in_a_block_and_attached_where_checks_are
- [x] an_unknown_review_is_an_error_wherever_it_is_attached
- [ ] misplaced_reviews_are_errors
- [x] a_review_plans_every_job_whatever_its_verdicts
- [ ] the_spitout_records_only_where_each_review_file_is `drop` (#418 removes the `.spitout`)
- [x] potential_dag_carries_review_scope_and_ignores_the_sheet_contents
- [ ] an_inventory_round_trip_preserves_the_review_file_reference `drop` (#418 removes the `.spitout`)
- [x] a_recipe_names_only_reviews_the_pipeline_declares
- [ ] exclude_from_is_not_a_recipe_rule_and_says_what_to_write `drop` (migration diagnostic for an old spelling)
- [x] a_review_is_declared_in_a_pipeline_and_named_in_a_recipe
- [ ] an_import_brings_the_reviews_its_operations_and_sources_attach

### `scalars.rs`: port

- [ ] a_scalar_is_a_part_naming_its_file_and_bounds
- [ ] a_quoted_scalar_is_text
- [ ] a_scalar_in_verify_reads_the_same_file
- [ ] a_misused_scalar_says_what_to_write
- [ ] a_scalar_reads_one_input_file
- [ ] a_scalar_of_a_folder_is_an_error
- [ ] a_check_cannot_read_a_scalar

## Tools, resources and streams (30)

### `redirects.rs`: port

- [ ] a_job_names_its_stdin_and_stdout_apart_from_its_arguments
- [ ] the_plan_shows_a_redirected_stream_as_a_shell_writes_it
- [ ] a_redirection_names_one_port_of_the_right_kind
- [ ] stdin_is_one_file_and_stdout_is_not_a_folder
- [ ] only_an_operations_command_redirects
- [ ] a_redirected_output_changes_the_jobs_commands

### `resources.rs`: port

- [ ] a_job_carries_its_cores_and_memory_and_its_command_the_cores
- [ ] resources_leave_the_commands_unless_the_command_uses_them
- [ ] the_plan_shows_what_a_job_needs_above_its_command
- [ ] a_malformed_line_says_what_to_write
- [ ] cores_is_a_number_a_command_or_verify_takes
- [ ] an_operation_carried_out_by_steps_takes_no_resources
- [ ] an_imported_operation_brings_its_resources

### `src/inputs/tools.rs`: merge

- [ ] a_tool_is_a_text_or_a_probe
- [ ] a_comment_ends_a_line_and_is_not_part_of_an_identity
- [ ] mistakes_are_on_their_lines

### `src/model/tools.rs`: merge

- [ ] the_first_name_replaces_every_tool_and_later_names_add_up
- [ ] a_removal_alone_keeps_every_other_tool

### `tools.rs`: port

- [ ] with_no_uses_every_job_depends_on_every_tool
- [ ] lists_add_up_from_the_file_through_each_stage_to_the_operation
- [ ] a_mistake_in_a_list_is_reported_where_it_is_written
- [ ] the_spitdag_lists_each_tool_and_each_job_names_its_own
- [ ] a_recipe_s_spitenv_must_list_every_declared_tool
- [ ] a_spitenv_is_checked_alone_and_through_its_recipe
- [ ] without_a_spitenv_jobs_depend_on_no_tool
- [ ] an_anchored_operation_marks_its_jobs_and_a_wrong_name_is_an_error
- [ ] uses_and_anchored_may_come_before_the_operation_they_name
- [ ] a_removal_means_the_same_wherever_it_is_in_a_list
- [ ] each_compile_error_of_tool_uses_and_anchored_names_its_place
- [ ] an_imported_operation_keeps_its_tools_and_mark_and_the_importer_cannot_change_them

## DAG, CLI output and reports (87)

### `artifacts.rs`: port

- [ ] keeps_complete_jobs_and_blocks_consumers_of_incomplete_ones
- [ ] a_numeric_near_miss_points_to_the_existing_artifact
- [ ] resolve_fails_with_the_first_gap_the_report_finds
- [ ] a_job_lists_every_gap_it_has
- [ ] a_small_collection_leaves_other_groups_complete
- [ ] a_coverage_gap_holds_back_its_sources_and_blocks_their_consumers
- [ ] a_source_no_job_reads_is_listed_as_unused
- [ ] an_invalid_inventory_is_still_an_error
- [ ] render_lists_complete_then_incomplete_artifacts
- [ ] diagnose_still_reports_the_gap_as_an_error
- [ ] cli_lists_incomplete_artifacts_where_check_fails
- [ ] by_target_nests_what_each_final_target_waits_on
- [ ] by_target_gives_each_target_its_own_tree_and_shows_an_artifact_once_under_it
- [ ] by_target_on_a_complete_dataset_has_no_targets_section
- [ ] by_target_names_an_artifact_two_branches_wait_on_where_it_is_shown
- [ ] by_target_names_an_artifact_a_job_and_the_job_it_waits_on_both_need
- [ ] by_target_writes_an_upstream_gap_once_for_all_the_targets_that_share_it
- [ ] by_target_stops_indenting_past_twenty_levels
- [ ] by_target_output_grows_with_the_length_of_a_chain_not_its_square `bench` (timing claim: belongs in profiling)

### `cli.rs`: port

- [ ] help_lists_each_step_and_each_command_explains_itself
- [ ] keywords_prints_the_keyword_table_as_json_and_help_leaves_it_out `drop` (the editor contract is dropped; #423)
- [ ] check_compiles_the_pipeline_without_its_inputs
- [ ] dag_resolves_a_pipeline_over_a_spitout `drop` (#418 removes the `.spitout`)
- [ ] plain_dag_prints_counts_and_commands_are_explicit
- [ ] dag_counts_the_jobs_of_each_step_and_keeps_empty_steps
- [ ] partial_dag_plans_the_complete_stores_and_records_the_rest
- [ ] a_partial_plan_leaves_out_an_aggregate_with_any_member_that_cannot_be_made
- [ ] artifacts_by_target_groups_the_weekly_stores_under_their_final_target
- [ ] partial_on_complete_inputs_has_the_same_jobs
- [ ] dag_with_paths_displays_resolved_paths_before_command_expansion
- [ ] expanded_examples_resolve
- [ ] path_rules_report_fallbacks_and_check_accepts_them
- [ ] each_option_applies_to_its_commands
- [ ] check_json_reads_the_pipeline_file_and_dag_json_emits_the_spitdag `port` (only the DAG half; `check --json` is dropped)
- [ ] dag_json_names_every_output_port
- [ ] dag_json_stage_is_an_array_of_names
- [ ] path_rules_reject_an_output_with_no_rule
- [ ] recipe_path_rules_show_combined_coverage_and_origin
- [ ] a_recorded_source_with_no_path_rule_is_not_looked_for_under_out
- [ ] a_message_names_the_file_it_is_about_when_that_file_was_not_given
- [ ] check_prints_every_diagnostic_and_fails_only_on_errors
- [ ] a_recipe_names_its_own_pipeline_for_dag_and_artifacts
- [ ] a_recipe_run_in_memory_prints_each_pipeline_warning_once
- [ ] a_recipe_checks_a_pipeline_saved_with_a_byte_order_mark `merge` (one byte-order-mark case per entry point)
- [ ] drop_rules_that_remove_every_group_stop_each_command

### `examples.rs`: port

- [ ] worked_patterns_resolve_to_the_documented_jobs
- [ ] examples_have_no_diagnostics
- [ ] a_recipe_and_the_spitout_beside_it_settle_the_same_jobs `drop` (#418 removes the `.spitout`)

### `feature_interactions.rs`: port

- [ ] a_missing_source_stops_the_plan_and_names_what_it_holds_back
- [ ] a_partial_plan_leaves_out_a_job_with_its_beside_output_and_the_aggregate_that_needs_it
- [ ] a_scan_and_its_records_settle_to_the_same_plan

### `outputs.rs`: port

- [ ] a_complete_dataset_prints_as_before
- [ ] command_lines_print_as_before
- [ ] a_dataset_with_gaps_prints_as_before
- [ ] recipes_diagnosed_in_memory_match_their_text `drop` (tests a Rust library boundary, not behaviour)
- [ ] recipe_source_paths_and_reviews_diagnose_in_memory_as_in_text `drop` (tests a Rust library boundary, not behaviour)

### `src/blob.rs`: port (the runner reads `pipeline_files` blob ids; the OCaml writer emits none yet)

- [x] ids_match_what_git_hash_object_prints
- [x] hashing_in_place_matches_hashing_one_padded_buffer

### `src/json.rs`: port

- [ ] values_are_written_compactly_with_escapes
- [ ] escapes_are_found_anywhere_in_long_text
- [ ] numbers_are_written_in_decimal

### `src/model/artifacts.rs`: drop (Rust table internals)

- [ ] adding_an_artifact_twice_gives_its_id_and_keeps_one_row
- [ ] a_product_is_numbered_once_and_keeps_the_first_type_it_is_given
- [ ] find_gives_what_add_added_and_nothing_else
- [ ] ids_count_up_from_zero_in_the_order_artifacts_are_added

### `src/render/topology.rs`: drop (the `dag --tree` renderer is dropped)

- [ ] dependencies_precede_readers_even_when_declared_later
- [ ] stage_overview_keeps_named_dependencies
- [ ] repeated_joins_with_a_shared_product_stay_narrow
- [ ] shared_products_have_one_connected_rail
- [ ] three_inputs_have_three_separate_arrows
- [ ] two_inputs_join_into_a_vertical_chain
- [ ] branches_and_joins_show_named_products
- [ ] repeated_inputs_and_stages
- [ ] isolated_products_and_inputless_steps
- [ ] long_chains_stay_narrow

### `src/render/topology_connected.rs`: drop (the `dag --tree` renderer is dropped)

- [ ] crossing_preserves_an_unrelated_product_line

### `src/render/topology_wide.rs`: drop (the `dag --tree` renderer is dropped)

- [ ] folding_keeps_each_product_on_a_separate_continuous_line
- [ ] reversed_bands_keep_labels_readable_and_reverse_horizontal_arrows

### `src/spitdag/mod.rs`: port

- [ ] a_folder_is_reached_from_another_as_written

### `src/spitdag/write/tests.rs`: port

- [ ] a_spitdag_is_written_with_its_version_and_escapes
- [ ] a_job_writes_one_or_several_commands_with_their_streams
- [ ] a_removal_is_written_with_its_rule_and_why
- [ ] external_inputs_are_in_the_order_many_inputs_take

### `topology_cli.rs`: drop (the `dag --tree` renderer is dropped)

- [ ] topology_needs_no_inventory
- [ ] reusable_and_nested_operations_have_one_component_diagram
- [ ] mrtrix_examples_render_narrow_stage_panels
- [ ] combined_tractography_connects_shared_outputs_once

## Properties and scale (38)

### `mutation_fuzz.rs`: port

- [ ] a_damaged_pipeline_never_panics
- [ ] a_damaged_recipe_never_panics
- [ ] damaged_records_never_panic
- [ ] damaged_types_never_panic

### `robustness.rs`: port

- [ ] a_path_inside_another_artifacts_file_is_rejected
- [ ] a_path_rule_that_cannot_name_a_file_says_why
- [ ] paths_that_differ_only_in_case_are_flagged
- [ ] a_source_value_holding_its_rules_separator_is_flagged
- [ ] shell_operators_in_a_command_are_errors
- [ ] quoted_operators_and_a_shell_script_stay_arguments
- [ ] a_bare_lowercase_input_names_an_untyped_port
- [ ] duplicate_input_ports_are_named
- [ ] a_byte_order_mark_is_ignored `merge` (one byte-order-mark case per entry point)
- [ ] an_import_must_be_a_regular_file
- [ ] json_mode_always_prints_json `drop` (the editor contract is dropped; #423)
- [ ] command_line_mistakes_are_named
- [ ] a_byte_order_mark_is_ignored_by_every_entry_point `merge` (one byte-order-mark case per entry point)
- [ ] deeply_nested_type_arguments_are_an_error_not_a_crash

### `scale.rs`: port

- [ ] a_long_chain_of_steps_compiles_without_overflowing_the_stack
- [ ] calls_nested_ten_thousand_bodies_deep_lower_without_overflowing_the_stack
- [ ] many_sources_resolve_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_bad_selectors_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_calls_to_a_misspelled_operation_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_calls_to_an_operation_that_failed_to_declare_are_left_out_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_bad_steps_that_end_bodies_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_bad_body_headers_and_nested_call_failures_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] rendering_diagnostics_costs_the_diagnostics_not_the_length_of_the_file
- [ ] bad_steps_in_a_body_cost_their_own_work_and_not_a_reading_of_the_file_each
- [ ] many_bad_steps_in_an_operation_body_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] a_body_with_every_step_failing_is_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_repeated_path_extension_and_check_lines_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_conflicting_imports_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_calls_that_clash_with_their_operations_body_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)
- [ ] many_stages_with_a_bad_first_line_or_a_bad_closing_line_are_reported_in_reasonable_time `bench` (timing claim: belongs in profiling)

### `scaling.rs`: bench

- [ ] each_stage_grows_in_step_with_the_dataset `bench` (timing claim: belongs in profiling)
- [ ] checking_grows_in_step_with_the_pipeline `bench` (timing claim: belongs in profiling)

### `src/check.rs`: bench

- [ ] checked_step_binding_scales_with_steps_and_definitions `bench` (timing claim: belongs in profiling)

### `src/compile/definitions.rs`: port

- [ ] a_long_chain_is_ordered_without_deep_recursion

## Repository policy and editor support (29)

### `architecture.rs`: port (as an OCaml check of module layers and file length)

- [ ] modules_use_only_their_layer_and_below
- [ ] rust_files_stay_short

### `builtin_words.rs`: drop (keyword documentation for the editor)

- [ ] words_are_found_by_where_they_are_written
- [ ] columns_count_utf16_units_and_skip_a_byte_order_mark `drop` (the editor contract is dropped; #423)
- [ ] every_word_is_documented_once_and_links_to_a_section
- [ ] check_writes_each_words_documentation_once
- [ ] a_recipe_and_a_spitout_have_words_too `drop` (#418 removes the `.spitout`)

### `docs_examples.rs`: defer

- [ ] every_spit_block_in_the_user_pages_checks_or_says_why_it_does_not

### `hovers.rs`: drop (the editor contract is dropped)

- [ ] recipe_inputs_hover_with_source_and_path_dimensions `drop` (the editor contract is dropped; #423)
- [ ] calls_specialise_generics_and_products_across_a_chain
- [ ] bindings_are_local_and_multi_outputs_have_their_own_types
- [ ] aggregate_hovers_show_inference_and_output_dimensions `drop` (the editor contract is dropped; #423)
- [ ] product_paths_report_explicit_nearest_stage_and_pipeline_rules_or_their_absence
- [ ] imported_symbols_and_command_references_have_precise_hovers `drop` (the editor contract is dropped; #423)
- [ ] broken_steps_do_not_claim_inferred_types_or_hide_independent_symbols
- [ ] hover_ranges_use_utf16_and_exclude_comments_and_literal_text `drop` (the editor contract is dropped; #423)
- [ ] cli_editor_analysis_is_opt_in_and_preserves_error_diagnostics `drop` (the editor contract is dropped; #423)
- [ ] hovers_keep_resolved_path_hints_and_extensions `drop` (the editor contract is dropped; #423)
- [ ] path_hints_keep_literal_braces_and_brackets_doubled
- [ ] beside_outputs_keep_their_path_and_signature_in_hovers `drop` (the editor contract is dropped; #423)
- [ ] a_source_shows_its_extension_and_no_stage_default
- [ ] composite_hovers_show_typed_ports_and_one_body_in_the_right_context `drop` (the editor contract is dropped; #423)
- [ ] primitive_calls_prioritise_bindings_and_keep_execution_information
- [ ] product_hovers_keep_inference_failures_and_bound_large_reader_lists `drop` (the editor contract is dropped; #423)
- [ ] call_hovers_explain_the_selectors_that_change_matching `drop` (the editor contract is dropped; #423)
- [ ] a_broken_composite_retains_its_contract_without_claiming_inferred_results
- [ ] steps_beneath_a_stage_header_being_written_keep_their_hovers `drop` (the editor contract is dropped; #423)

### `source_comments.rs`: port (as an OCaml check that sources are comment-free)

- [ ] rust_sources_have_no_comments
- [ ] distinguishes_comments_from_literals_and_lifetimes
