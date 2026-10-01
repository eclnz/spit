# Task: one leaderboard across a training sweep

An ML sweep is laid out under `data/`:

- `models/<model>.pt`: base checkpoints.
- `configs/<config>.yaml`: training configs.
- `sweep/<config>/seed-<seed>.json`: the seed files for that config. Configs do not all have the same seeds.
- `eval/testset.parquet`: the single held-out test set.

Plan these steps:

1. Train every base model with every config, once for each seed file of that config: `train --model <model.pt> --config <config.yaml> --seed-file <seed.json> --out <weights>` → `runs/<model>/<config>/seed-<seed>/weights.pt`
2. Evaluate every trained run on the test set: `evaluate --weights <weights> --testset <testset> --out <metrics>` → `runs/<model>/<config>/seed-<seed>/metrics.json`
3. For each model and config, summarise over its seeds: `summarise --out <summary> <metrics> ...` in seed order → `summaries/<model>/<config>.json`
4. Make one leaderboard over every model and config summary: `leaderboard --out <board> <summary> <summary> ...` → `leaderboard.csv`. The summaries are ordered first by model name, then by config name.

Adding a model, a config, or a seed file must need no change to the pipeline.
