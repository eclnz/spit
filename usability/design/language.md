# Design: language changes

This plan resolves [F6](../FINDINGS.md#f6-accept-a-source-with-no-dimensions), [F9](../FINDINGS.md#f9-aggregate-over-several-dimensions-in-one-step) and [F10](../FINDINGS.md#f10-smaller-language-requests).

## F6: sources with no dimensions

**Today.** A source must have a bracket list. `source testset : Data` fails with `expected product name followed by [dimensions]` (`src/parser/declarations.rs`), and the working form, `source testset : Data []`, appears nowhere in the guide. Both scenario 3 agents hit this, and so did the study's own answer key.

**Change.**

- **Parsing.** Accept a source declaration without brackets as a source with no dimensions, both with a type (`source testset : Data`) and without one (`source testset`). `[]` stays valid.
- **Display.** Write a product with no dimensions as its bare name (`testset`, `leaderboard`) in `dag`, `artifacts` and messages, instead of `testset[]`. A `.spitout` already accepts a bare record such as `source_lut`, so the two now agree.
- **Error.** Any source line that still fails says what is expected: `expected source name, optional ": Type", and optional [dimensions]`.

**Compatibility.** No declaration that parses today changes meaning. Stored outputs showing `name[]` are re-blessed.

**Tests.**

- Parser tests for the three forms, with and without a type.
- Resolution of a pipeline whose only source has no dimensions, joined to a source with dimensions: it must match every job.
- Re-bless `tests/fixtures/outputs/` where `[]` appears.

## F9: aggregating over several dimensions in one step

**Today.** `@ vary` and `@ drop` each take one dimension, and each may appear once:

- `InputBinding::vary: Option<String>` and `OperationDef::aggregated_dimension: Option<String>` in `src/model.rs`.
- The parser rejects a list, or a second clause, in `src/parser/declarations.rs` and `src/parser/operation.rs`.

So a leaderboard over every model and every config needs two steps, and scenario 3's brief had to be written around that.

**Change.**

- **Syntax.** Accept a list in both clauses. `@ vary(model, config)` on the call and `@ drop(model, config)` on the operation must name the same set of dimensions:

  ```text
  operation leaderboard(summaries: many Summary) -> Table @ drop(model, config)
  board = leaderboard(summary @ vary(model, config))
  ```

- **Model.** `vary` becomes a list (`Vec<String>`), and `aggregated_dimension` becomes `aggregated_dimensions: Vec<String>`. The builders `InputBinding::vary` and `OperationDef::aggregating` keep one-dimension forms for the tests and add list forms.
- **Grouping.** A group is the many input's dimensions minus every varied dimension. This is a set difference in place of the single filter in `step_driver` (`src/shape.rs`).
- **Checks.** The contract check in `src/compile/steps.rs` compares sets, and still reports a varied dimension the input lacks.
- **Order.** A collection is ordered by the product's dimensions in declared order, with values compared as `natural_cmp` compares them, as for one dimension now. With `summary [model, config]`, the order is by model, then config.
- **`@ min(n)`.** Counts the whole collection.
- **Errors.** `@ vary(...) takes one dimension` and `duplicate @ vary(...)` are replaced by a message that suggests the list form when a second clause is written: `write the dimensions in one clause: @ vary(model, config)`.

**Tests.**

- Scenario 3's original shape, one leaderboard over every model and config, resolved and ordered.
- Set mismatches in either direction.
- `@ min` over two dimensions.
- The one-dimension cases unchanged.

**Guide.** Replace the one-dimension wording under "Operations and commands" and show the leaderboard example.

## F10: smaller changes

### The call's `@ vary` follows from the operation's `@ drop`

**Today.** An aggregation is written twice: `@ drop(date)` on the operation and `@ vary(date)` on every call, and the two must agree. Two agents called this redundant.

**Change.** When an operation declares `@ drop(...)`, a call's many input may leave out `@ vary(...)`: the call varies the dropped dimensions. A call that writes `@ vary` must still name the same set, and a mismatch stays an error.

- The operation keeps its `@ drop`, because it is the contract that states the output's dimensions.
- An operation without `@ drop` still needs `@ vary` on the call.

**Where.** The contract check in `src/compile/steps.rs` fills a missing `vary` from the operation before checking. The driver logic in `src/shape.rs` then sees a complete binding.

### Named arguments in a call

**Current behavior.** Call arguments follow port order, although the driving input may be any port. One agent put the driving input first and got a type mismatch at another port.

**Decision.** Keep positional input order. Each argument is checked against its operation port's type, and a mismatch names that port and product. Named calls would weaken the visible order contract and add syntax for a low-priority mistake.

### Where an `each` dimension goes

No change in behaviour. A dimension broadcast with `@ each(...)` is placed after the driving input's dimensions (`step_context` in `src/shape.rs`), so `trained` is `[config, seed, model]`. This decides the order of `{entities}`. Document it under `each` (see [guide gaps](guide.md)).
