# Single-pass recovery plan

Make error recovery parse a document once, and decide what a line is by grammar rather than by guessing. This follows the parser layout work (eclnz/spit#132), which built a document's structure before its content but left two gaps against the compiler designs it followed:

- **Recovery parses again after errors.** Lowering stops at a statement that may have changed the builder, so `recover_document` leaves failed statements out (blanking some, passing others to the parser as skipped) and parses again until a pass is clean. #90 lists the cases that still cost a parse per error.
- **Line classification guesses.** A product may be named like a keyword, so `Keyword::split` has special cases (`root =`, `check :`, `stage =`, `default =`, `entities:`), and three rules decide what a step is (#131).

## Steps

1. [x] **Reserve the keywords** (`8cfaa5a`). A product, by source or by step, may not take a statement keyword's name. `Keyword::split` reads a keyword from the first word alone, and a step written with a keyword's name says the name is reserved. The layout gains `Head::Step`, the one rule for a step, which `inputs/mod.rs` and `parser/inventory.rs` use too (#131). The language reference lists the reserved words.
2. [x] **Lower each statement whole or not at all** (`b4a5527`). Every lowering failure leaves the builder as it was: `lower_body`'s failures already come before any change, `apply_import`'s conflicts are found before it adds anything, and `add_flow_step` rolls the builder back to where the statement began when a nested call fails. `Failure::clean` goes, and lowering never stops early.
3. [x] **Return a partial document with its errors** (`4c228b1`). Lowering gives the builder and every error. The checks after lowering run on what was lowered. A `use` that fails imports nothing, and its error stays. Records in a pipeline are an error that does not stop lowering. The inventory parser reads past a bad record.
4. [x] **Recover in one parse** (`4c228b1`). `recover_document` parses once and filters the errors that repeat another (`Failures::explains`). Blanking, `skipped`, and the `skipped` parameter through the parse functions go.
5. [ ] **One layout per document.** `split_document`, `read_imports` and `parse_flow` share the document's layout, with records marked rather than blanked out of a copy of the text.

Each step runs the checks in `AGENTS.md`. Valid files give the same bytes as before (other than in step 1, where a pipeline naming a product after a keyword is now an error). Invalid files are fuzzed against the commit before, and #90's cases are timed.
