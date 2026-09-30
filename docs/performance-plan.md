# Performance plan

SPIT's work should grow in proportion to a dataset: the files it scans, the
records it settles, the jobs it resolves. It did not. Several steps compared
every record or job with every other one, so a 1,000-subject study took
15 seconds to resolve and 5,000 subjects took minutes. The first part of this
plan, below, removed that and cut the constant costs. The rest is what
profiling still shows.

## How it is measured

The workload is the MRtrix3 ACT example (`examples/commands/mrtrix3_act/`) over
generated BIDS datasets: each subject has two sessions, each session two DWI
runs with their sidecars, a reversed-phase b=0 and a T1w. A 1,000-subject
dataset is 22,002 files and resolves to 60,000 jobs. The dataset is made with
this script (`python3 gen.py <folder> <subjects>`), and the recipe is
`mrtrix3_act_discover.spitin` with its `pipeline` line pointing at a copy of
`mrtrix3_act.spit`:

```python
import os, sys
root, n = sys.argv[1], int(sys.argv[2])
os.makedirs(f"{root}/config", exist_ok=True)
for f in ("source_lut.txt", "target_lut.txt"):
    open(f"{root}/config/{f}", "w").close()
for s in range(1, n + 1):
    for ses in (1, 2):
        sub, se = f"{s:05d}", f"{ses:02d}"
        d = f"{root}/sub-{sub}/ses-{se}"; stem = f"sub-{sub}_ses-{se}"
        for k in ("dwi", "fmap", "anat"):
            os.makedirs(f"{d}/{k}", exist_ok=True)
        for f in (f"fmap/{stem}_dir-PA_epi.nii.gz", f"fmap/{stem}_dir-PA_epi.json",
                  f"anat/{stem}_T1w.nii.gz"):
            open(f"{d}/{f}", "w").close()
        for run in ("01", "02"):
            for ext in ("nii.gz", "bvec", "bval", "json"):
                open(f"{d}/dwi/{stem}_run-{run}_dwi.{ext}", "w").close()
```

Times are wall-clock for a release build, the quicker of two runs; memory is
peak resident size. To find where time goes, profile a 300-subject run with
`valgrind --tool=callgrind` on a release build with debug symbols
(`CARGO_PROFILE_RELEASE_DEBUG=true`), and compare it with a 100-subject run:
a function whose cost grows 3 times is in proportion, one that grows 9 times
is quadratic.

`tests/scaling.rs` guards the shape: it times each stage over a small dataset
and one four times its size, and fails when a stage takes more than 9 times as
long. Before the first part of this plan, settling took 13.6 times as long
there; every stage now takes under 5.

## Where it stands

| Command, 1,000 subjects | Before | First round | Now |
| --- | --- | --- | --- |
| `spit inputs` | 5.1 s | 0.63 s | 0.38 s |
| `spit dag` over a `.spitout` | 14.5 s | 0.96 s | 0.45 s |
| `spit dag` over a recipe | 25.0 s | 2.0 s | 1.0 s |
| `spit dag --json` | 21.9 s | 2.3 s | 0.99 s |
| `spit artifacts` | 13.7 s | 0.56 s | 0.28 s |

At 5,000 subjects (110,002 files, 300,000 jobs), `dag` takes 3.3 s in 800 MB
and `dag --json` 6.9 s in 1.2 GB. After the first round they took 6.0 s and
16.3 s in 2.5 GB; before it, `dag --json` ran for minutes. Times on this
machine vary by a fifth from run to run.

## Done

Every change was checked against the code before it: the CLI's output on the
golden fixtures, every example, and the generated datasets was byte for byte
the same, and so was the library's on 13,000 random pipelines, record sets
and directory trees (resolution, lenient resolution, settling, diagnosis,
bound paths, the `.spitdag` and both text renderers), including cases built to
reach each error whose choice depends on order.

- **Quadratic work removed.**
  - Resolving matches each job's inputs through an index by join values rather than scanning every candidate.
  - `require` and `skip` checks group a product's records in one pass rather than once per group.
  - Skipped groups are looked up rather than scanned, in settling and discovery.
  - Writing a `.spitout` looks contexts up in a set.
- **Work done twice removed.** `dag` and `artifacts` reuse the resolution their diagnosis made, which `diagnose_checked_with_records` returns as `Records`, rather than resolving again.
- **Artifacts are cheap to copy and find.**
  - An `EntityBinding` shares its map and keeps its hash.
  - The resolver and path binding find artifacts in hash maps by product and entities (`ArtifactMap`), not ordered maps of owned keys.
- **Allocation and writing costs cut.**
  - Path components, JSON strings and artifact names are written without allocating per byte or per character.
  - JSON values borrow their text, and a job's fingerprint is hashed as it is written.
  - The `.spitdag` is written a job at a time.
- **Discovery tests fewer patterns.** It rejects a path pattern whose literal start or end a file lacks before searching.

In the second round, where a well-used library does the job it replaces
code of our own:

- **mimalloc is the CLI's allocator.** SPIT makes and frees many small strings; allocation was a quarter of a run with the system allocator.
- **rustc-hash's `FxHashMap` and `FxHashSet`** replace the standard hasher in maps whose order does not matter: artifact maps, the join index, path owners, bindings' own hashes, and the duplicate check in `source_artifacts`, which compared whole entity maps in a `BTreeSet` about fifteen times per record.
- **serde_json writes all JSON**, the `.spitdag` and `check --json`, streamed to the output. One formatter keeps the text as it was, so fingerprints are unchanged; the `fnv` crate computes them.
- **Jobs share their bound artifacts.** `bind_dag` binds each artifact once and every job that uses it holds the same `Arc<BoundArtifact>`: a quarter fewer allocations, and less memory to walk when writing.
- **Paths bind with each product's template and stage found once** (`PathBinder`), rather than once per artifact, in `bound_paths` and in discovery.

## What is left

Profiled at 300 subjects, `dag` over a `.spitout` now executes 1.3 billion
instructions (6.5 billion before the first round). In order of what each
would save:

### 1. Settle a `.spitout` once

`dag` checks the inventory in its diagnosis (`check_inventory`), then settles
it again in `prepare` with `InputSpec::resolve`: about a tenth of `dag`.
`check_inventory` also clones the whole inventory to apply skip rules, even
when there are none. The diagnosis could return what it settled, as it
returns what it resolved.

### 2. Diagnose a recipe's records in memory

`dag recipe.spitin` settles the dataset, writes the `.spitout` text, parses it
back and diagnoses that, so it takes twice as long as `dag` over the written
`.spitout`. Diagnosing the settled inventory directly would drop the text
round trip and a second settle; errors about records would still need a place
to point, which today is a line of that text.

### 3. Print jobs without building their lines

`render_dag` builds a `Line` with owned strings for every artifact of every
job, and formats each artifact's type again: about a fifth of `dag`. Writing
each line as it is formatted, with each product's type formatted once, would
remove most of it.

### 4. Resolve with fewer copies

`expand_step` is a quarter of `dag`. Each job holds its own copy of every
input artifact, and each copy allocates its product name and type. Sharing
them (`Arc<str>`, an `Arc` around the type), or having jobs refer to artifacts
by index, would cut the copies and the memory: at 300,000 jobs, `dag` still
peaks near 600 MB.

### 5. Bind command arguments without copies

After sharing artifacts, most of `bind_dag`'s allocations are command
arguments: each is a `Vec` of parts that own their text and paths. Sharing the
literal text and paths would cut them, at the cost of changing `ArgPart`.

### 6. Warn about paths that differ only in case without binding again

The diagnosis binds every path to look for case collisions, and `bind_dag`
binds them again: about a seventh of `dag`. The two differ in general (the
diagnosis uses the pipeline as written and sources not yet located), so
reusing one for the other needs care.

### 7. Group records without copying their bindings

`coverage_gaps`, `rejected_groups` and `SkipIndex` project every record onto
the rule's dimensions to find its group, building a new map per record per
rule. Grouping by the borrowed values, and building a binding only for each
distinct group, would make this a fraction of that. The order groups are
reported in must stay the order of their bindings.

### 8. Match source file paths with less bookkeeping

After the literal start and end checks, discovery's `match_from` still
allocates for every attempt it remembers and recomputes which dimensions
later pieces repeat. The repeated dimensions can be found once per pattern.

### 9. Key a DAG's source paths without copies

`ResolvedDag::source_paths` is a public `BTreeMap<ArtifactKey, String>`, so
finding a source's file builds an owned key and compares strings. Moving it
to a map found by borrowed product and entities changes the public type.

### 10. A faster fingerprint

A fingerprint is FNV-1a over the job's work, serialized a second time just
to be hashed, one dependent multiply per byte: about a third of writing the
`.spitdag`. A hash that takes eight bytes at a time would be several times
faster, but changes every fingerprint, so a backend would see every job as
changed once. It belongs with a `.spitdag` version change.
