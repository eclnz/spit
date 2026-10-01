# Design: small CLI and file fixes

This plan resolves [B4](../rounds/1/README.md#b4-no-command-shows-the-full-set-of-path-rules), [B6](../rounds/1/README.md#b6-external_inputs-is-in-text-order) and [B7](../rounds/1/README.md#b7-spit-help-promises-a-script). Each is one small, independent commit.

## B4: show every path rule, whichever file holds it

**Today.**

- `spit check recipe.spitin --path-rules` is refused (`src/main.rs`): `--path-rules and --strict-paths check a pipeline, not a recipe`.
- `spit check pipeline.spit --path-rules` marks a source whose rule the recipe supplies as `MISSING` (`src/paths/rules.rs`).

So no command shows the rules a recipe and its pipeline make together, and the one that comes closest reports a problem that is not there.

**Change.**

- **On a recipe.** `check recipe.spitin --path-rules` merges the recipe's source rules into the pipeline, as resolving does already (`with_source_paths` in `src/inputs`), and prints the combined coverage. Each rule gets its origin:

  ```text
  Product path coverage:
    t1w (source): explicit sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.nii.gz (recipe)
    bold (source): explicit sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz (recipe)
    mc: explicit derivatives/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_mc.nii.gz
  ```

  `--strict-paths` on a recipe checks the combined rules.
- **On a pipeline alone.** A source with no rule is shown as `no rule (a recipe may supply one)`. An output product with no rule and no default keeps `MISSING`, since only the pipeline can give it one.

**Tests.** Replace the refusal test in `tests/cli.rs` with one for the combined listing, and add one for the new source wording.

## B6: order a `.spitdag`'s lists as `many` inputs are ordered

**Today.** `external_inputs` is sorted by path as plain text (`BoundDag::external_inputs`, `src/spitdag.rs`), so waves come out as `1, 10, 2`. Every `many` placeholder uses `natural_cmp`, in which runs of digits compare as numbers, and gives `1, 2, 10`.

**Change.** Sort `external_inputs` by `natural_cmp` on the path, falling back to plain comparison so the order stays total. `targets` stays in job order, which already follows the resolver.

This is not a change to the format, so `SPITDAG_VERSION` stays at 3.

**Tests.** Add a unit test in `src/spitdag.rs` with waves `1`, `2` and `10`, and re-bless any stored `.spitdag` output.

## B7: the help line

**Today.** `spit help` begins "spit: compile a pipeline, settle a dataset's inputs, resolve jobs, and write a script". No command writes a script.

**Change.** Change it to "spit: compile a pipeline, settle a dataset's inputs, and resolve its jobs into a .spitdag" (`src/main.rs`). Update the help test in `tests/cli.rs` if it matches the text.
