# First pipeline

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
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines
command merge: sort -m -u -o {@output} {items}
merged = merge(sorted @ vary(part))
```

`shard[group=alpha,part=01]` is one source artifact. The first step makes a `sorted` artifact for each shard. `@ vary(part)` gathers the sorted shards of each `group` into one `merged` artifact. The complete example also gives outputs a [default path rule](paths.md).

## 3. Check it and inspect a plan

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit --path-rules
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout --counts --commands
```

The checked-in `.spitout` lists two `alpha` shards and one `beta` shard. The plan has three sort jobs and two merge jobs. `--counts` shows the number per step; `--commands` shows the filled command arguments before anything runs.

## 4. Save the plan

```sh
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout -o /tmp/command_demo.spitdag
```

The `.spitdag` is the job description a [runner](https://github.com/eclnz/spit-bash) reads. See the [DAG format](../manual/dag.md) if you are writing a runner.

## 5. Use a real dataset

When source files are on disk, give SPIT a dataset root. A recipe makes that root and any dataset rules repeatable:

```spit
pipeline analysis.spit
root data
```

Run `spit inputs dataset.spitin` to see the source artifacts SPIT finds, then `spit dag dataset.spitin --counts --commands` to inspect the jobs. If source paths already live in the pipeline and no recipe rules are needed, use `spit dag analysis.spit --root data` instead. Read [recipes and input inventories](recipes.md) before adding `discover`, `exclude`, or `require` rules.

Next: [Products](products.md).
