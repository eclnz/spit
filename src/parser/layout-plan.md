# Parser layout plan

Build a document's structure (its lines, statements and blocks) from the text alone, before any statement's content is parsed, so that a line that fails to parse can't change how the lines after it are read. This is the parser refactor that eclnz/spit#130 (a `default` keyword, keyword blocks and multi-line declarations) builds on.

## Why

`parse_flow` works out block structure (stages, operation bodies, continued signatures) in the same pass as it parses each line's content. When a line fails, the failure can change that structure, and error recovery is defined as "read as if the failed line were blank". Keeping that definition true is what these exist for:

- `Statement::stateful`, `Failure::clean` and `defer_parse_errors` in `src/lower/mod.rs`;
- the `closed` and `ended_body` watches in `parse_flow` and `OpenStages::fixed_by` in `src/parser/flow.rs`;
- the statement counts in `Syntax::errors`;
- `recover_document` in `src/diagnostics/recovery.rs`, which parses the document again after each error, and keeps its own copy of the continuation rule (`signatures`) and of which lines are a body's (`body_line`, `Failures::above`).

Besides the parser, these modules also strip comments and classify raw lines themselves: `src/imports.rs` (`read_imports`), `src/inputs/mod.rs` (`header_lines`, `check_input_lines`), `src/parser/inventory.rs` (`split_document`, `parse_inventory_with_lines`), `src/builtins.rs` (`builtin_words`), `src/parser/continuation.rs` and `src/diagnostics/recovery.rs`. They don't all agree: `inputs/mod.rs` and `parser/inventory.rs` each have their own `is_step`, with different rules.

## Design

**Layout** (`src/parser/layout.rs`) is built from the text alone:

- each physical line, with its indent, the code without its comment, and what it starts with (`Head`: blank, a `Keyword`, a records `Header`, or something else);
- (step 2) logical statements, which join physical lines by one continuation rule;
- (step 3) a block tree: a flat table of statements in source order, each with its parent and the end of its subtree, so a block's children are a slice and nothing recurses.

A statement opens a block if it looks like one from the surface: `stage …:`, `operation …:` with the colon after its brackets close, or (with #130) a bare `keyword:`. For any other statement, a deeper-indented line continues it.

**Content.** `parse_flow` walks the layout and parses each statement's text. When one fails, the error is attached to that statement and the structure stays as it was.

## Steps

1. [x] **Physical lines** (`65de752`). Added `Layout` with each line's indent, code, comment and `Head`. Moved `read_imports`, `header_lines`, `check_input_lines`, `split_document`, `parse_inventory_with_lines`, `builtin_words`, `operation_lines` and recovery's line reading onto it, and removed `Keyword::of`. Each reader keeps its own rules where they differed, such as the two `is_step`s. Output is byte-for-byte the same as `dev` on every example and fixture. `parse_flow` and `body.rs` still read logical lines themselves; steps 2 and 3 move them. `split_document` and `parse_inventory_with_lines` each build a layout of their own text for now; sharing one layout per document comes with step 2.
2. [ ] **Logical statements.** Join lines in `Layout`, keeping today's rule (an `operation` with open parentheses), and delete `continuation.rs` and recovery's `signatures`. Nothing a user sees changes.
3. [ ] **Block tree.** Build stages and operation bodies in `Layout`, and have `parse_flow` walk them in place of `Open` and `OpenStages`. Nothing changes for a valid file.
4. [ ] **Structure before content.** Remove `stateful`, `Failure::clean`, `defer_parse_errors`, the watches in `parse_flow` and, for pipelines, the reparse loop in `recover_document`. This changes recovery on purpose: a `stage` line with lines indented beneath it opens a block whatever its content. Review each changed expectation in `tests/syntax_errors.rs` and `tests/diagnostics.rs` rather than re-saving them all.
5. [ ] **The language changes in #130.** With the structure settled first, these become mostly rules in `Layout`.

Each step runs the checks in `AGENTS.md`, including `profiling/bench.py pipeline` and `dataset` against the commit before, since parsing counts as performance-related code.

## Open questions

- A multi-line statement's text: keep it as one slice of the original text, newlines and comments included, and have content parsers treat a newline as a space and strip comments line by line. This would replace the joined `Cow` and the address arithmetic in `ParseError::locate`.
- Whether to separate syntax nodes from model types (`ProductDef` and others in `StatementKind`). This is only needed for a formatter, not for #130.
- Whether the two `is_step`s should become one. Merging them changes how some unusual lines are read, so it belongs in its own commit with its own tests.
