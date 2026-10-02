# Profiling

`bench.py` times the `spit` CLI on generated pipelines and datasets, and
profiles it with callgrind. Use it to check a change against the commit
before it for a slowdown, and to find where the time goes when a stage is
slow.

The rules that keep SPIT's work in step with its data are under
[Performance](../docs/architecture.md#performance) in the architecture notes.

It needs Python 3 and a release build; `profile` also needs valgrind.
Generated pipelines, datasets and profiles go in `profiling/work/`, which
git ignores. Datasets there are reused between runs.

```sh
cargo build --release
python3 profiling/bench.py pipeline          # spit check on 2000 steps
python3 profiling/bench.py dataset           # spit inputs, dag, check on 1000 subjects
python3 profiling/bench.py profile --steps 1000
```

Each time is the quickest of three runs (`--repeats`), in milliseconds.
The defaults are one size of each workload, so a run takes seconds: enough
to catch a regression, such as a step that has become quadratic, not to
measure how a stage scales. For that, give more sizes, as in
`--steps 1000,2000,4000` or `--subjects 1000,4000 --extra 0,60`.

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

With `--old`, each of the new build's times is compared with the old
build's, and the command exits with status 1 when one is more than 1.3
times the old (`--tolerance`) and more than 5 ms slower, a difference small
times show from run to run:

```text
slower than 1.3x the old build:
  2000 steps check: 27.0 -> 85.8 ms (3.17x)
```

Times vary by 10% or more on a shared machine, so run a failed check again
before taking it as a regression. The `new+ext` row of `dataset` has no old
row to compare with.

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
steps. `PipelineIndex` in `src/model/pipeline.rs` finds each producer once. The
dataset stages were unchanged within noise.
