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

| Command, 1,000 subjects | Before | Now |
| --- | --- | --- |
| `spit inputs` | 5.1 s | 0.63 s |
| `spit dag` over a `.spitout` | 14.5 s | 0.96 s |
| `spit dag` over a recipe | 25.0 s | 2.0 s |
| `spit dag --json` | 21.9 s | 2.3 s |
| `spit artifacts` | 13.7 s | 0.56 s |

At 5,000 subjects (110,002 files, 300,000 jobs), `inputs` takes 3.9 s, `dag`
6.0 s in 778 MB, a recipe 13.0 s, and `dag --json` 16.3 s in 2.5 GB. Before,
`dag --json` ran for minutes and passed 2.3 GB.

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

## What is left

Profiled at 300 subjects, `dag` over a `.spitout` now executes 1.7 billion
instructions (it was 6.5 billion), `dag --json` 4.0 billion and `dag` over a
recipe 3.1 billion. In order of what each would save:

### 1. Write JSON through a buffer, not the formatter

`dag --json` takes 2.3 times as long as `dag`. The JSON writer is generic
over `fmt::Write`, but the `.spitdag` is written through a `Formatter`, so
every comma, quote and key is a dynamic call: about 45 instructions per byte
of output. Writing each job into a reused `String` and handing the formatter
one piece per job should bring that near the cost of copying the bytes.

### 2. Bind each artifact's path once per run

`bound_paths` binds every artifact of every job, and a `dag` run can call it
three times: for the case-collision warning in the diagnosis, for
`validate_source_files` with `--root`, and in `bind_dag` for `--json`, `-o`
and `--paths`. Each call is about 18% of `dag`. Within it, `bind_path` looks
up the product's template and stage again for every artifact (about 7,000
instructions each), and the map that detects shared paths copies every path.
Binding once, with the template and stage found once per product, and passing
the paths on would remove most of this.

### 3. Settle a `.spitout` once

`dag` checks the inventory in its diagnosis (`check_inventory`), then settles
it again in `prepare` with `InputSpec::resolve`, which checks it a second
time: about 18% of `dag`. The diagnosis could return what it settled, as it
now returns what it resolved.

### 4. Diagnose a recipe's records in memory

`dag recipe.spitin` settles the dataset, writes the `.spitout` text, parses it
back and diagnoses that, so it takes twice as long as `dag` over the written
`.spitout`. Diagnosing the settled inventory directly would drop the text
round trip and a second settle; errors about records would still need a place
to point, which today is a line of that text.

### 5. Group records without copying their bindings

`coverage_gaps`, `rejected_groups` and `SkipIndex` project every record onto
the rule's dimensions to find its group, which builds a new map per record per
rule: 460 million instructions for the three `require` rules of a 300-subject
recipe. Grouping by the borrowed values, and building a binding only for each
distinct group, would make this a fraction of that. The order groups are
reported in must stay the order of their bindings.

### 6. Print jobs without building their lines

`render_dag` builds a `Line` with owned strings for every artifact of every
job before writing it: about 15,000 instructions per job, 17% of `dag`.
Writing each line as it is formatted would remove the copies.

### 7. Use less memory for large DAGs

At 300,000 jobs, `dag` peaks at 778 MB and `dag --json` at 2.5 GB. Each job
holds its own copy of every input artifact, including its product name and
type, and the bound DAG copies each again as owned strings, with its path.
Options, from least to most change to the library's API:
- share product names and types between artifacts (`Arc<str>`, an `Arc` around `TypeExpr`);
- write the `.spitdag` straight from the resolved DAG and its paths, without a `BoundDag`;
- have jobs refer to artifacts by index.

### 8. Read source records once per run

`Pipeline::source_artifacts` turns records into artifacts, copying each
product name and type and checking for duplicates in an ordered set. A `dag`
run calls it several times: in each `check_inventory` and in resolving.
Items 3 and 4 remove most calls; the remaining one could share names and
types, as in item 7.

### 9. Match source file paths with less bookkeeping

After the literal start and end checks, discovery's `match_from` still
allocates for every attempt it remembers (a list of bound values per attempt)
and recomputes which dimensions later pieces repeat: 12% of `dag` over a
recipe. The repeated dimensions can be found once per pattern, and patterns
that repeat none need no list.

### 10. Key a DAG's source paths without copies

`ResolvedDag::source_paths` is a public `BTreeMap<ArtifactKey, String>`, so
finding a source's file builds an owned key and compares strings. Moving it
to a map found by borrowed product and entities changes the public type.
