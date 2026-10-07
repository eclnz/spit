# Ragged sweep

Each configuration owns its seeds: `fast` has 1 and 2, while `deep` has only 1. The `seed[config,seed]` artifacts drive training. `model @ each(model)` broadcasts each model over those **observed** config/seed pairs; it does not manufacture `deep` seed 2. No source holds both `model` and `config`, so nothing says which comes first; `dimensions [model, config, seed]` declares it once for the whole pipeline. Every product then lists its dimensions in that order, so `trained` is `[model, config, seed]`, `summary` is `[model, config]`, and the final `many Summary` collection sorts by model, then config. Without the line, `spit check` stops at `trained` and suggests it.

Save as `sweep.spit`:

```spit
# Each config has its own seeds; every model is tried with every seed.
# No source orders `model` and `config`, so the pipeline declares it.
dimensions [model, config, seed]
source model : Weights [model]
path model: models/{model}.pt
source config : Config [config]
path config: configs/{config}.yaml
source seed : Seed [config, seed]
path seed: seeds/{config}/{seed}.json
source testset : Data
path testset: eval/testset.parquet
operation train(model: Weights, config: Config, seed: Seed) -> Weights
command train: train --model {model} --config {config} --seed {seed} --out {@output}
path trained: runs/{model}/{config}/{seed}/weights.pt
trained = train(model @ each(model), config, seed)
operation evaluate(weights: Weights, testset: Data) -> Metrics
command evaluate: evaluate {weights} {testset} --out {@output}
path metrics: runs/{model}/{config}/{seed}/metrics.json
metrics = evaluate(trained, testset)
operation summarise(runs: many Metrics) -> Summary
command summarise: summarise {runs} --out {@output}
path summary: summaries/{model}/{config}.json
summary = summarise(metrics @ vary(seed))
operation leaderboard(summaries: many Summary) -> Table
command leaderboard: leaderboard {summaries} --out {@output}
path board: leaderboard.csv
board = leaderboard(summary @ vary(model, config))
```

Save as `sweep.spitout`:

```text
sources:
    model[model=small]
    model[model=large]
    config[config=fast]
    config[config=deep]
    seed[config=fast,seed=1]
    seed[config=fast,seed=2]
    seed[config=deep,seed=1]
    testset
```

Run `spit dag sweep.spit sweep.spitout --commands` to see 17 jobs: six `train`, six `evaluate`, four `summarise`, and one `leaderboard`. For `config=deep`, there are two training jobs, one per model, both with seed 1. The final `leaderboard` command receives summaries in this order: `large/deep`, `large/fast`, `small/deep`, `small/fast`. Run `spit dag sweep.spit sweep.spitout -o sweep.spitdag` to save the plan. The input paths in this inventory are illustrative; add the named files under the paths declared above if you want SPIT to verify their existence with `--root`.

Next: [Cohort](cohort.md).
