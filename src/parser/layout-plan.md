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
- (step 3) a block tree: a flat table of statements in source order, each with its parent block and the block it opens, built with an explicit stack.

A statement opens a block if it looks like one from the surface: a `stage` line, an `operation …:` with the colon after its brackets close, or (with #130) a bare `keyword:`. With #130, a deeper-indented line after any other statement continues it.

**Content.** `parse_flow` walks the layout and parses each statement's text. When one fails, the error is attached to that statement and the structure stays as it was.

## Steps

1. [x] **Physical lines** (`65de752`). Added `Layout` with each line's indent, code, comment and `Head`. Moved `read_imports`, `header_lines`, `check_input_lines`, `split_document`, `parse_inventory_with_lines`, `builtin_words`, `operation_lines` and recovery's line reading onto it, and removed `Keyword::of`. Each reader keeps its own rules where they differed, such as the two `is_step`s. Output is byte-for-byte the same as `dev` on every example and fixture. `parse_flow` and `body.rs` still read logical lines themselves; steps 2 and 3 move them. `split_document` and `parse_inventory_with_lines` each build a layout of their own text for now; sharing one layout per document comes with step 2.
2. [x] **Logical statements** (`bc9f564`). `Layout::statements` groups the lines into statements, keeping today's rule (an `operation` with open parentheses). `parse_flow` and recovery's `signatures` read them, and `continuation.rs` is gone. Nothing a user sees changes. Each reader still builds a layout of the text it is given: `split_document` passes the parser a copy with the records blanked, and recovery reparses a blanked copy after each error, so one layout per document waits on the reparse loop (step 3).
3. [x] **Block tree, and structure before content** (`ee458ac`, steps 3 and 4 together). They could not land apart: a block opened only when its header parsed, so building blocks from the layout changes recovery for invalid files. `Layout::statements` now gives each statement its parent block and the block it opens (`Block::Stage` for any `stage` line, `Block::Body` for an `operation` header ending in `:`). `parse_flow` walks that in one pass; `Open`, `OpenStages`, the `closed` and `ended_body` watches, `Statement::stateful` and `defer_parse_errors` are gone, and `flow.rs` went from 677 lines to 531. The rules settled while building it:
   - A failed line still closes the blocks it is outdented from.
   - A block whose header fails keeps its lines, read in the stage around it as ordinary statements. `dev` read them as if the header were blank, which put them in whatever block came before at that indentation.
   - A stage's or body's indentation is set by its first line that parses, as before, so one bad line does not make the rest "indented differently".
   - A line that can start no statement (no keyword, no `=`) neither opens nor closes a block, so a stray line does not move the lines after it out of their block.
   - Valid files: byte-for-byte the same as `dev` on every example and fixture. Invalid files: 4,200 damaged copies of the examples, fuzzed through both builds, never disagreed on whether a file is valid and never panicked. The differences were reviewed. Most are improvements (a stray line reports its own error, a broken stage's lines keep the right stage, fewer cascades); a few are the agreed rule that a failed line still closes its block. Three tests in `tests/syntax_errors.rs` changed for that reason, and three were added.
   - **Not done, and why:** `Failure::clean` and the reparse loop in `recover_document` stay. Lowering stops at a statement that may have changed the builder, and recovery's reparse also finds errors in imported libraries, misplaced records and inventory lines; a one-pass experiment failed nine tests across those. Recovery now passes the parser a set of failed block headers to skip, rather than blanking them, so their blocks keep their lines on the next pass. Removing the loop needs lowering to be atomic per statement, which is outside this plan.
4. [ ] **The language changes in #130.** With the structure settled first, these become mostly rules in `Layout`.
   - [x] The `default` keyword (`29099cd`): `default path:`, `default ext:`, `default check:` and the `default:` block (`Block::Defaults`). The old `path:`, `ext:` and `check:` lines are errors that say how to write them; there are no users to keep them for.
   - [x] Keyword blocks and statements over several lines (this commit, see below). `source:`, `operation:`, `command:`, `verify:`, `check:`, `path:`, `require:` and `exclude:` open blocks (`Block::Keyword`), whose entries the parser reads as that keyword's statements; an `operation:` entry ending in `:` opens its body. Any statement continues while its brackets are open. A deeper-indented line continues only a command (`command`, `verify` or a `check` declaration), and never when it starts a statement of its own: fuzzing showed that continuing every declaration let a mis-indented step or operation vanish into the line above it.

Each step runs the checks in `AGENTS.md`, including `profiling/bench.py pipeline` and `dataset` against the commit before, since parsing counts as performance-related code.

## Open questions

- A multi-line statement's text: keep it as one slice of the original text, newlines and comments included, and have content parsers treat a newline as a space and strip comments line by line. This would replace the joined `Cow` and the address arithmetic in `ParseError::locate`.
- Whether to separate syntax nodes from model types (`ProductDef` and others in `StatementKind`). This is only needed for a formatter, not for #130.
- The two `is_step`s become one `Head::Step`: eclnz/spit#131.
