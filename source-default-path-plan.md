# Source default path plan

A pipeline has one default path rule, and it does two jobs: it finds sources and places outputs. The two rarely share a layout. Sources sit where the dataset keeps them, such as BIDS raw `sub-01/anat/...`; outputs go where SPIT writes, often by stage. A default such as `path: {@stage}/{@product}/{@entities}` fails for every source, since no source is made in a stage, so each source needs its own rule, which goes against the point of a default.

## Design

- **Where a source is found belongs to the dataset, so its default goes in the recipe.** A recipe's `path:` line is the default rule for every source with no rule of its own, in the pipeline or the recipe. The pipeline's `path:` keeps placing outputs, and still finds sources when the recipe sets no default, as before.
- Precedence for a source: its pipeline rule, else its recipe rule, else the recipe's default, else its stage's or the pipeline's default.
- The recipe's default is used as written: `ext:` belongs to the pipeline and completes output paths, so it does not complete the recipe's default. A source with another extension gets its own rule.
- `{@stage}` in a recipe's default is an error, since no source is made in a stage.
- When the recipe meets its pipeline, the default is expanded into one rule per source it covers. A `.spitout` then writes each under `source_paths:` as it does a recipe's own rule, so it needs no new syntax and resolves without the recipe.
- `check --path-rules` on a recipe marks such a source `default ... (recipe)`, and `--strict-paths` rejects it as it does any default.

## Steps

- [x] When a rule's bare `{@stage}` fails for a product outside every stage, say to write `[{@stage}/]`.
- [ ] Let a recipe's `path:` be the default rule for its sources: parse, expand against the pipeline, label in `--path-rules`, reject `{@stage}`.
- [ ] Tests, the language reference, the README and `docs/architecture.md`.
- [ ] Check whether `spit-vscode` needs a change for `path:` in a `.spitin`.
- [ ] Delete this plan in its own commit before the work merges.
