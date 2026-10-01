# Profiling

`bench.py` times the `spit` CLI on generated pipelines and datasets, and
profiles it with callgrind. Use it to compare a change with the commit
before it, and to find where the time goes when a stage is slow.

The rules that keep SPIT's work in step with its data are under
[Performance](../docs/architecture.md#performance) in the architecture notes.

It needs Python 3 and a release build; `profile` also needs valgrind.
Generated pipelines, datasets and profiles go in `profiling/work/`, which
git ignores. Datasets there are reused between runs.

```sh
cargo build --release
python3 profiling/bench.py pipeline          # spit check on 1000, 2000, 4000 steps
python3 profiling/bench.py dataset           # spit inputs, dag, check on 1000 and 4000 subjects
python3 profiling/bench.py profile --steps 1000
```

Each time is the quickest of five runs (`--repeats`), in milliseconds.

## The workloads

- **pipeline**: a chain of steps, each reading the one before. It finds
  work that grows with products × steps, such as looking up each product's
  producer by searching every step. It times `spit check` plain, with
  `--json`, and with `--json --hovers`.
- **dataset**: the scaling test's pipeline (`tests/scaling.rs`) over a
  dataset of subjects, each with a mask and two sessions of a reference and
  two runs, plus `--extra` steps after its four. It times `spit inputs`,
  `spit dag` from the `.spitout`, and `spit check`, and once more with
  `ext:` completing the default path.
- **profile**: one `spit check` on a chain under callgrind, printing the
  `spit` functions that take the most instructions, inclusive. Give `spit
  check` more arguments after `--`, as in `profile -- --json --hovers`.

## Comparing with an earlier commit

Build the earlier commit in a worktree, then pass it as `--old`:

```sh
git worktree add ../spit-old <commit>
(cd ../spit-old && cargo build --release)
python3 profiling/bench.py pipeline --old ../spit-old/target/release/spit
git worktree remove ../spit-old
```

A build from before `{@product}` is given the old `{product}` spelling, and
skips `--hovers` and `ext:`, which it does not have.

## Line-level profiles

A release build keeps function names but not line numbers. For a profile
that `callgrind_annotate` can annotate line by line, build with debug
information into a separate target folder and profile that build:

```sh
CARGO_PROFILE_RELEASE_DEBUG=true CARGO_TARGET_DIR=target/profiling cargo build --release
python3 profiling/bench.py profile --new target/profiling/release/spit
```

## Results so far

On the cloud container this was first run on, `spit check` took, in ms:

| steps | `158db65` | after the product index |
|---|---|---|
| 1000 | 185 | 18 |
| 2000 | 502 | 29 |
| 4000 | 2015 | 72 |

At `158db65` each product's path checks searched every step for the
product's producer, so checking took time in proportion to products ×
steps. `PipelineIndex` in `src/model.rs` finds each producer once. The
dataset stages were unchanged within noise.
