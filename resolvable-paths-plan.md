# Check that paths resolve, not that they are explicit

`spit check --strict-paths` is the only check that every product has a path, and it also rejects every product that takes a default, so it fails pipelines such as `examples/commands/mrtrix3_act` whose paths all resolve. Plain `check` passes a recipe that leaves a source without a rule, which `spit inputs` then rejects. And the built-in output path, `out/{@product}/{@entities}`, is given to sources too, so a `.spitout` source with no rule is looked for under `out/`.

## Steps

- [x] Give the built-in output path to outputs only, where the pipeline reads its rules, instead of setting it as the pipeline's default in four places. `--path-rules` shows it as `built-in default`. Commit: "Check that paths resolve, and remove --strict-paths".
- [x] A record of a source with no path rule is an error, not a file under `out/`. Commit: "Check that paths resolve, and remove --strict-paths".
- [x] `spit check recipe.spitin` fails a source that no rule covers: its own, the recipe's, or a default. Commit: "Check that paths resolve, and remove --strict-paths".
- [x] Remove `--strict-paths`, now that `check` checks what it should have. Commit: "Check that paths resolve, and remove --strict-paths".
- [x] Update the README, language reference and architecture notes; run the checks. Commit: "Check that paths resolve, and remove --strict-paths".
- [ ] Delete this plan in its own commit before the work merges.
