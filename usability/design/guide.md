# Design: closing the guide's gaps

This plan resolves the [guide gaps](../FINDINGS.md#guide-gaps), D1–D11.

**Where changes go.** The guide an agent reads is the README's user-facing sections followed by `docs/language-reference.md`, so every change here lands in one of those two files.

**Two kinds of gap.** Some gaps close with the change that alters the behaviour concerned, in the same commit; the repository's rule is to update the guide with the behaviour. The rest describe behaviour that is staying as it is, and need only writing down.

## Gaps closed by a planned change

| Gap | Closed by |
| --- | --- |
| D8: how `skip` and `require` form groups | [Removing inputs](removing-inputs.md): the rewritten Recipes section defines groups for `drop` and `require` |
| D5 (partly): unmatched files are skipped silently | [Diagnostics](diagnostics.md): the unmatched-file note and `--unmatched` |
| D6 (partly): the `[]` form | [Language, F6](language.md#f6-sources-with-no-dimensions): brackets become optional |

## Gaps to write down now

### D1: the smallest recipe

**Section:** Recipes, and "Supply the inputs" in the README.

Show that a recipe may be one line: `pipeline analysis.spit` finds every source by its path rule, with no `discover` rule. Say when `discover` is worth adding: when sources expand over directories that must exist, or when a directory should count as a context even if it is empty. Lead the section with this form, before `discover`.

### D2: where files live

**Section:** a new section of the README, "Where files live", between "Supply the inputs" and "Resolve jobs".

- The dataset root is the recipe's folder, or `--root` when given.
- Every source and output path is relative to that root.
- `--root` on `dag` and `artifacts` sets that root as well as checking that files exist. Change the CLI table's wording for `--root` to say so.
- The `pipeline` line of a recipe is relative to the recipe's folder, and may use `..`. `tests/inputs.rs` already relies on this.
- Two layouts, with the commands for each:
  - the pipeline and recipe beside the data, with no `--root`;
  - the pipeline elsewhere, with the recipe in the data folder or `--root` naming it.

### D3: the `.spitdag` format

**Section:** a new reference page, `docs/spitdag.md`, linked from the README's "Resolve jobs".

Describe each field of version 3, as `src/spitdag.rs` writes it:

- `version` and `generator`.
- `root`: absolute when known, `null` otherwise.
- `external_inputs` and `targets`, and how each is ordered.
- `executables`.
- Each job's fields: `id`, `operation`, `stage` (a list, outermost first), `fingerprint` (what it covers, and that it changes when the command or its inputs do), `inputs` and `outputs` by port, `depends_on` and `dependents`, `command` and `verify`.
- The command encoding: a list of arguments, each a list of parts that are strings or `{"path": …}` objects, joined without separators.

Point to `dag --commands` ([commands view](commands-view.md)) as the readable form.

### D4: how collections are ordered

**Section:** "Operations and commands", where `many` inputs are described.

State the rule `natural_cmp` implements (`src/model.rs`):

- Values are compared dimension by dimension, in the product's declared order.
- Within a value, runs of digits compare as numbers and other characters compare one by one, so `run=2` comes before `run=10`.
- ISO dates (`2026-09-01`) therefore sort by date.
- Names sort by character (`lr-high`, `lr-low`, `warmup`).
- `1` and `01` are equal as numbers; which comes first is then decided by comparing the texts.

### D5: path rules match whole paths

**Section:** Paths.

State that a source rule matches a file's whole path from the root. So `wave{wave}.csv` does not match `wave3.csv.bak` or `wave3.csv.1`, and files that match no rule are ignored. Link to the unmatched-file note once it exists.

### D6: products with no dimensions

**Section:** Products and dimensions, and Paths.

- Declare one as `source testset : Data`, or `[]` until [F6](language.md#f6-sources-with-no-dimensions) lands.
- As an input, such a product matches every job, with no selector.
- Its path rule is a fixed path, such as `path board: leaderboard.csv`.
- It is shown by its bare name.

### D7: source rules in the pipeline or the recipe

**Section:** Recipes.

A source's path rule may be written in the pipeline or in the recipe, but not both; SPIT reports a source with rules in both.

- **In the pipeline:** a layout every dataset for that pipeline shares.
- **In the recipe:** a layout particular to one dataset.

### D9: what `verify` means

**Section:** Operations and commands, and `docs/spitdag.md`.

SPIT writes `verify` commands into each job and does not run them; a backend does:

- Every `verify` command must succeed before the job's command runs.
- If one fails, the job fails, and so does every job that depends on it.
- In the `.spitdag`, `verify` is a list of commands in the same encoding as `command`.

### D10: names

**Section:** Operations and commands.

- Products, operations and dimensions each have their own namespace, so `model @ each(model)` and a product named like its operation are both valid.
- An untyped `many` port is written `many items` or `items: many`; both parse. `many` alone is also accepted, and its placeholder is `{inputs}`.
- A dimension broadcast with `@ each(...)` is placed after the driving input's dimensions, which decides the order of `{entities}`.

### D11: worked examples

**Section:** new example pipelines under `examples/`, each with a `.spitin` and a `.spitout`. `tests/examples.rs` checks every pipeline under `examples/` already. List each in `docs/examples.md`, and link each from the reference section it illustrates.

| Example | Shows | Reference section |
| --- | --- | --- |
| `examples/patterns/archive_revision` | Choosing one revision from an archive with `where` | Selectors |
| `examples/patterns/per_group_reference` | One file per group whose name differs, with `same` | Selectors |
| `examples/patterns/model_fit` | A `many` input, two outputs, `@ drop` and `verify` in one step | Operations and commands |
| `examples/patterns/ragged_sweep` | Models × configs × per-config seeds, with `each` then `vary` | Selectors (`each`) |
| `examples/patterns/cohort` | BIDS sessions, one subject dropped, one run excluded | Recipes (after [removing inputs](removing-inputs.md)) |

The study's answer keys (`usability/harness/scenarios/*/key`) are working versions of four of these, and can be adapted, keeping them out of the harness so trial agents never see them.

## Order

1. **D1–D4, D7, D9, D10 now,** in one commit. They describe behaviour that is not changing.
2. **D5, D6 and D8** with the changes that close them.
3. **D11** last, so the cohort example uses `drop` and `exclude`.

Then re-run the study. A gap is closed when no run asks about it.
