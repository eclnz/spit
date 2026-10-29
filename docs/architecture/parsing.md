# Parsing and lowering

Text in, a `Pipeline` and every error out. A recipe also gives its `InputRules` and any records it writes. This stage decides a document's structure and its declarations.

```mermaid
flowchart LR
    text["document text"] --> layout["Layout<br/>lines and statements"]
    layout --> uses["use lines<br/>Import::read"]
    layout --> syntax["Syntax<br/>parse_flow"]
    layout --> records["SourceInventory<br/>document_records"]
    uses --> lower["lower"]
    syntax --> lower
    lower --> document["ParsedDocument<br/>Pipeline, InputRules, SourceMap"]
    records --> document
    lower --> errors["Vec of ParseError"]
    errors --> recovery["recover_document"]
```

## Data

Each box of the diagram, the function that makes it, and what the box doesn't show:

1. **`Layout`**: `Layout::new` ([`parser/layout.rs`](https://github.com/eclnz/spit/blob/dev/src/parser/layout.rs)). Each `Line` has its indentation, its code without its comment, its `Head`, and whether it is a record. Each `Statement` has its lines, its parent, and the `Block` it opens.
2. **`Syntax`**: `parse_flow` ([`parser/flow.rs`](https://github.com/eclnz/spit/blob/dev/src/parser/flow.rs)). The statements that parsed, and a `ParseError` for each that did not, with how many statements came before it.
3. **Imports**: `Import::read` ([`imports.rs`](https://github.com/eclnz/spit/blob/dev/src/imports.rs)). The `Pipeline` each `use` line brings in, by line.
4. **`lower`** ([`lower/mod.rs`](https://github.com/eclnz/spit/blob/dev/src/lower/mod.rs), [`lower/expand.rs`](https://github.com/eclnz/spit/blob/dev/src/lower/expand.rs)) gives a `PipelineBuilder`, from which the document takes its `Pipeline`, `InputRules` and `SourceMap`.
5. **Records**: `document_records` ([`parser/inventory.rs`](https://github.com/eclnz/spit/blob/dev/src/parser/inventory.rs)), for a recipe only.
6. **`ParsedDocument`**: `parse_layout` ([`lower/mod.rs`](https://github.com/eclnz/spit/blob/dev/src/lower/mod.rs)).
7. **Recovery**: `recover_document` ([`diagnostics/recovery.rs`](https://github.com/eclnz/spit/blob/dev/src/diagnostics/recovery.rs)) drops the errors that only repeat another.

The entry points differ in what they read and what they return:

- `parse_pipeline` reads a pipeline with no imports. `parse_pipeline_at` reads one at a path, and resolves its `use` lines relative to that path (`parse_located_document_recovering`).
- `parse_input_spec` and `parse_input_spec_at` read a recipe. They first take out its `pipeline` and `root` lines (`header_lines` in `src/inputs/mod.rs`), blanking them so that the other lines keep their numbers.
- The `Result` entry points return the first error. The `_recovering` functions return the document with every error, which `src/diagnostics` and the editor read.
- `Kind` says which file is being read. In a `Kind::Pipeline`, a `discover`, `require` or `exclude` rule is an error, and so are records.

## Responsibilities

- **`Layout` decides structure, and nothing else.** It reads each line's indentation and its first word, and from those alone it works out:
    - what each line starts with (`Head`): blank, a statement's `Keyword`, a records `Header`, a step, or something else;
    - which lines make each statement. A statement goes on while its parentheses are open. A `command`, `verify` or `check` statement also takes the lines indented beneath it.
    - which statements open a block, and which block holds each statement. The blocks are a `stage`, an operation body, `default:`, and a keyword alone with its colon, such as `source:`.
    - which lines are records: from a `sources:` or `contexts:` header to the next statement or step.

    Every reader of a document's lines takes them from a `Layout`, so they agree on what each line is. Within one parse, the `use` lines, statements and records all come from the same one (`parse_document_at_inner`, `parse_layout`).

- **`parse_flow` parses each statement's content,** in the block that `Layout` gave it. A statement that fails gives a `ParseError` and is left out.
- **`lower` builds the `Pipeline`.** It:
    - checks that each name is declared once, and before it is used;
    - expands each call to an operation carried out by steps into those steps (see [calls](#calls-to-operations-carried-out-by-steps));
    - infers the dimensions of the products a step makes;
    - puts every product's dimensions in the pipeline's order (`order_dimensions` in `src/order.rs`);
    - runs the checks that need the whole document, on what it lowered: `check_bodies_have_no_commands`, `check_source_beside_paths`, and the `{@entities}` spellings.
- **What lowering keeps for later stages:**
    - **Stages.** Each `Invocation` records the `stage` whose block holds it. The resolver keeps it once per step, in `DagStep::stage`. `Pipeline::stages` keeps one `StageDef` per name, with its default path rule, `default ext:` and `default entities:`. A nested stage's name is its path, such as `preprocess/combine`. A stage opened again continues its first block.
    - **A stage groups steps and scopes paths. It is not a namespace.** An operation declared in one is global. `SourceMap::operation_stages` records its stage only so that a warning can flag a call from outside it.
    - **Extensions.** A source's `ProductDef` carries the extension it declares. An operation's `OutputPort` carries its optional extension and, for an output its tool writes beside another output or an input, a `Beside`: that sibling, its suffix, and whether the suffix replaces the sibling's extension or, written `+`, follows its whole name.
    - **Companion sources.** A companion source keeps its main source and suffix in `ProductDef::beside`. `Pipeline::sidecar_groups` keeps each main source with its companions, so that the input stage can report a group found only in part.
    - **Collection minimums.** `@min(n)` on a `many` port lowers to `OperationDef::minimum_collection`. An operation takes at most one `many` input, so the minimum belongs to the operation.
- **Imports.** Each `use` line's library is parsed in turn, and the definitions it selects are merged into the caller.
- **Not done here:**
    - checking each step against its operation, types and cycles, which `collect_pipeline` does (see [resolution](resolution.md));
    - checking path rules, which `paths` does;
    - anything that needs a dataset.

## Invariants

- **Every line is in exactly one statement.** Test: [`each_line_is_read_once_with_its_head`](https://github.com/eclnz/spit/blob/dev/src/parser/layout.rs).
- **Structure comes before content.** A statement that fails to parse never changes how the lines after it are read:
    - a failed line still closes the blocks it is outdented from;
    - a block whose header fails keeps its lines, which are read as statements of the block around it;
    - a stage's first line sets the indentation of its lines, even when that line fails.

    Tests: [`a_line_that_fails_still_closes_the_stage_it_is_outdented_from`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs), [`a_stage_header_that_fails_keeps_its_lines`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs), [`the_steps_of_a_body_whose_header_fails_are_read_as_ordinary_statements`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs), [`the_first_line_of_a_stage_sets_its_indentation_even_when_it_fails`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs), [`blocks_come_from_indentation_and_the_opening_line`](https://github.com/eclnz/spit/blob/dev/src/parser/layout.rs).

- **A stray line moves nothing.** A line with no keyword and no `=` can start no statement. It fails where it stands, and neither opens nor closes a block (`Statement::stray`). Test: [`a_line_that_fails_still_ends_the_body_before_it`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs).
- **There is one rule for a step,** `is_step`, which every reader takes from `Head::Step`. Test: [`a_step_is_names_then_a_call`](https://github.com/eclnz/spit/blob/dev/src/parser/layout.rs).
- **Records are blank to statements.** Lines a `sources:` or `contexts:` header starts are read only by the inventory reader. Test: [`records_are_marked_and_blank_to_statements`](https://github.com/eclnz/spit/blob/dev/src/parser/layout.rs).
- **A statement is lowered whole or not at all.** Every failure comes before the builder changes, except a call whose expansion fails part way, which `add_flow_step` rolls back with `PipelineBuilder::journal`. So the statements after a failed one lower as they would without it. Tests: [`each_bad_step_of_a_body_is_an_error_and_the_operation_keeps_the_rest`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs), [`a_body_with_every_step_failing_is_reported_by_its_steps_alone`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs).
- **A pipeline of any length parses and lowers without overflowing the stack.** Test: [`a_long_chain_of_steps_compiles_without_overflowing_the_stack`](https://github.com/eclnz/spit/blob/dev/tests/scale.rs), which runs on request.
- **Damaged text never panics.** Test: [`tests/mutation_fuzz.rs`](https://github.com/eclnz/spit/blob/dev/tests/mutation_fuzz.rs).
- **Calls nest to any depth.** `add_flow_step` expands a call with a worklist, not recursion, so a call whose bodies nest 10,000 deep lowers without overflowing the stack. Test: [`calls_nested_ten_thousand_bodies_deep_lower_without_overflowing_the_stack`](https://github.com/eclnz/spit/blob/dev/tests/scale.rs), which runs on request.
- **The cost of errors grows with the errors, not the file.** Tests: the `..._in_reasonable_time` cases in [`tests/scale.rs`](https://github.com/eclnz/spit/blob/dev/tests/scale.rs), which run on request.

## Failure

- A statement that fails is left out whole, and its `ParseError` is placed at its line (`ParseError::within`). Lowering goes on with the next statement.
- `lower` returns the syntax errors and the lowering errors in the order of their statements, then the errors of the whole-document checks.
- A `use` line that fails imports nothing.
- Records in a pipeline are one error, `ParseErrorKind::MisplacedRecords`, however many lines they span. Test: [`records_in_a_pipeline_are_one_error_however_many_lines`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs).
- `recover_document` sorts the errors by line, and leaves out an error that only repeats another:
    - a call to an operation whose declaration or `use` line failed;
    - a body left with no steps because each of its steps failed.

    Test: [`calls_to_an_operation_that_failed_to_declare_repeat_its_error`](https://github.com/eclnz/spit/blob/dev/tests/syntax_errors.rs).

How the errors that remain are ordered, placed and shown is described under [diagnostics](diagnostics.md).

## Calls to operations carried out by steps

An operation whose header ends in `:` keeps its body in `OperationDef::steps`, each a `BodyStep` with its place. `src/parser/body.rs` opens the body. A call to such an operation reaches the `Pipeline` only as the steps it expands to, so `compile`, the resolver and binding see ordinary steps.

```mermaid
flowchart LR
    call["a call, as written"] --> expand["add_flow_step<br/>worklist"]
    body["OperationDef::steps"] --> expand
    expand --> steps["Invocation per body step<br/>with origin and checks"]
    expand --> table["Pipeline::calls<br/>Call by CallId"]
```

- **The body is checked where it is declared** (`check_body`, in `src/lower/expand.rs`). Its steps call operations declared before it, read only its ports and earlier steps' products, and assign each named output once.
- **Each body step becomes a step over the caller's products:**
    - a port reads the caller's argument, with the caller's selectors merged with the body's own;
    - an output is the product the caller names;
    - a product the body makes for itself is filed under the call's first output, as `first::product`, which `{@product}` writes `first.product`. So two calls never share an identity or a path.
- **Each call is a `Call` in `Pipeline::calls`, by `CallId`.** It holds the operation, the products the call reads and writes, its parent call and its place.
- **Each step a call makes carries `Invocation::origin`,** the call and the body step's place. It also carries in `Invocation::checks` the checks that the body attaches to the ports it reads and writes. Only the views, the diagnostics and the `.spitdag` read the origin.
- **An import brings an operation with its body whole,** with the operations and checks its steps use under the same prefix. `OperationDef::file` names the file that declares it, and `Pipeline::files` lists each file read, with its git blob id (`src/blob.rs`).
- **Each step is type-checked against its own operation,** which says nothing of the types the called operation promises. `check_calls`, in `src/compile/types.rs`, checks those (see [resolution](resolution.md)).

## Trade-offs

- **Structure from the surface alone.** Because a line's content never decides structure, one bad line cannot move the lines after it. The cost is that `Layout` must classify a line from its first word, which is why statement keywords are reserved: no product may take one's name.
- **Some passes lay the text out again.** `recover_document` does, and reads a failed line's text to find the operation it declares, which keeps the parser free of recovery state. A recipe's `header_lines` and `check_input_lines` do too.
