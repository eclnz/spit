# Getting started

This walkthrough uses the checked-in [text processing pipeline](https://github.com/eclnz/spit/blob/dev/examples/commands/command_demo/command_demo.spit). It plans one sort job per input shard and one merge job per group. The commands can be inspected without running `sort`.

## 1. Build SPIT

Install a stable [Rust toolchain](https://www.rust-lang.org/tools/install), then run from the repository root:

```sh
cargo build
```

The executable is `target/debug/spit`. The commands below use `cargo run --` so you need not add it to `PATH`.

## 2. Read the pipeline

The essential lines are:

```spit
root command_demo_data
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines
command merge: sort -m -u -o {@output} {items}
merged = merge(sorted @ vary(part))
```

`shard[group=alpha,part=01]` is one source artifact. The first step makes a `sorted` artifact for each shard. `@ vary(part)` gathers the sorted shards of each `group` into one `merged` artifact. The complete example also gives outputs a [default path rule](guide/paths.md).

## 3. Check it and inspect a plan

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit --path-rules
cargo run -- dag examples/commands/command_demo/command_demo.spit --counts --commands
```

The inline root and source path find two `alpha` shards and one `beta` shard in the checked-in data folder. The `.spitout` beside the pipeline records the same inputs. The plan has three sort jobs and two merge jobs. `--counts` shows the number per step; `--commands` shows the filled command arguments before anything runs.

## 4. Save the plan

```sh
cargo run -- dag examples/commands/command_demo/command_demo.spit -o /tmp/command_demo.spitdag
```

The `.spitdag` is the job description a [runner](https://github.com/eclnz/spit-bash) reads. See the [DAG format](spitdag.md) if you are writing a runner.

## 5. Use a real dataset

For a small pipeline, set `root data` and source path rules in the `.spit`, then run `spit inputs analysis.spit` or `spit dag analysis.spit`. The folder is relative to the pipeline file. Add a recipe when selection or coverage rules are needed:

```spit
pipeline analysis.spit
root data
```

Run `spit inputs dataset.spitin` to see the source artifacts SPIT finds, then `spit dag dataset.spitin --counts --commands` to inspect the jobs. If the pipeline already declares its root, omit the recipe's `root` line to inherit it. Declaring a root or the same source path in both files is an error. A pipeline with no root may take `--root data`, relative to where the command runs. Read [recipes and input inventories](guide/recipes.md) before adding `discover`, `exclude`, or `require` rules.

Next: [Files and flow](guide/concepts.md).
