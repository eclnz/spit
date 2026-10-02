# Source default path plan

A pipeline has one default path rule, and it does two jobs: it finds sources and places outputs. The two rarely share a layout. Sources sit where the dataset keeps them, such as BIDS raw `sub-01/anat/...`; outputs go where SPIT writes, often by stage. A default such as `path: {@stage}/{@product}/{@entities}` fails for every source, since no source is made in a stage, so each source needs its own rule, which goes against the point of a default.

## Design

- **Where a source is found belongs to the dataset, so its default goes in the recipe.** A recipe's `path:` line is the default rule for every source with no rule of its own, in the pipeline or the recipe. The pipeline's `path:` keeps placing outputs, and still finds sources when the recipe sets no default, as before.
- Precedence for a source: its pipeline rule, else its recipe rule, else the recipe's default, else its stage's or the pipeline's default.
- `ext:` belongs to the pipeline and completes output paths, so it does not complete the recipe's default. A source completes any rule ending without an extension with the one it declares, as `source events .tsv [sub]`.
- `{@stage}` in a recipe's default is an error, since no source is made in a stage.
- When the recipe meets its pipeline, the default is expanded into one rule per source it covers. A `.spitout` then writes each under `source_paths:` as it does a recipe's own rule, so it needs no new syntax and resolves without the recipe.
- `check --path-rules` on a recipe marks such a source `default ... (recipe)`, and `--strict-paths` rejects it as it does any default.

## Steps

- [x] When a rule's bare `{@stage}` fails for a product outside every stage, say to write `[{@stage}/]`. Commit: "Say to write [{@stage}/] when a product outside every stage meets {@stage}".
- [x] Let a recipe's `path:` be the default rule for its sources: parse, expand against the pipeline, label in `--path-rules`, reject `{@stage}`. A pipeline default that needs `{@stage}` no longer covers sources. Commit: "Let a recipe's path: be the default rule for its sources".
- [x] Tests, the language reference, the README and `docs/architecture.md`. Commit: "Let a recipe's path: be the default rule for its sources".
- [x] Check a recipe's source rules, its default's included, in `spit check`, which only `--path-rules` did: a default without `{@product}` passed `check` and failed in `inputs`. Commit: "Check a recipe's source paths in spit check, each at its line".
- [x] Check whether `spit-vscode` needs a change for `path:` in a `.spitin`. None: its one grammar already highlights `path:` in every file, and a recipe gets no inline path hints.
- [x] Keep the recipe as parsed data and derive each source's rule with pure functions, in place of settling the recipe in place behind a flag, following the data-oriented rules in `docs/architecture.md`. Commit: "Derive a recipe's source rules from its data instead of settling it in place".
- [x] Let a source declare its files' extension after its type, `source events .tsv [sub]`, as an operation's output and a `sidecars` member do, so one recipe default covers sources of different formats; it completes a source's own rule and any default, and a rule ending in another extension is an error. Fix a hover that showed an empty pipeline default for a source a stage default no longer covers. Commit: "Let a source declare its extension, so one default covers sources of different formats". `spit-vscode`: "Highlight the extension any source declares, before its dimensions".
- [ ] Delete this plan in its own commit before the work merges.
