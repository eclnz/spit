# Review snapshot (eclnz/spit#257, eclnz/spit#273)

Part of the quality-control theme, eclnz/spit#276, which puts this before
screens (#203's phase 2). One pull request, one `.spitdag` version: SPIT
records what its reviews withheld, and spit-bash reports it.

## What is wrong

SPIT applies reviews when it makes the plan and notes each rejection and
held artifact on stderr, but the `.spitdag` carries none of it. A runner
given the saved DAG sees only the jobs that remain, so a run that made the
masks for a review looks like a complete analysis though every fit was
withheld.

## `.spitdag` version 14

A new top-level field, `reviews`, after `left_out`: one entry per review
the pipeline declares and attaches to a product, in declaration order.

```json
{
  "name": "looked_at",
  "file": "/data/study/qc/looked_at.csv",
  "found": true,
  "verdicts": 2,
  "unmatched": [6, 7],
  "targets": [TARGET, ...]
}
```

- `file`: the review's file as an absolute path, or `null` when the recipe
  names none.
- `found`: whether the file existed when SPIT made the plan. A file that
  exists with a header and no rows is `found` with `verdicts` 0.
- `verdicts`: how many rows the file holds.
- `unmatched`: the lines of rows that match no artifact.
- `targets`: each artifact the review withholds, in the order the resolver
  meets them:

```json
{
  "artifact": ARTIFACT,
  "port": {"operation": "fit", "port": "mask"},
  "verdict": "fail",
  "reason": "ghosting",
  "line": 3,
  "withholds": ["fits[sub=02]", "reports[sub=02]"],
  "shrinks": ["result"]
}
```

- `artifact`: the reviewed artifact with its path, a source or an output of
  a job in the DAG, which a person inspects.
- `port`: where the review is attached when on an input port; `null` for
  a source or an output.
- `verdict`: `"fail"` for a rejection, `null` for one not yet judged.
- `reason`, `line`: from the row that failed it; `null` when held.
- `withholds`: the outputs not planned because of it, directly or through
  other withheld outputs, by identity as `left_out` writes them.
- `shrinks`: the outputs of `many` jobs planned without it.

Passed artifacts are not listed: they are planned as usual. A DAG made from
a `.spitout` has the same snapshot, so the `.spitout` records each review
file (below).

## SPIT

- **Resolver.** Keep, for each withheld artifact, the targets that withheld
  it (a target's index), so a left-out job's outputs inherit its inputs'
  causes. Record `withholds` and `shrinks` on each target as jobs are left
  out or lose members. `ArtifactReport::withheld` becomes the targets.
- **Inputs.** Record each recipe review file, found or not, with its path,
  in the inventory; the `.spitout` writes it as a line of its `verdicts:`
  section (`looked_at from qc/looked_at.csv`, `... (missing)`), so a DAG
  made from a `.spitout` knows the file and whether it existed.
- **Binding.** A target that no job reads (a withheld source) still gets its
  path, so a person can find it.
- **Notes.** A review whose file holds a header and no rows gets a note,
  as a missing file does. The review file is not listed as a dataset file
  that matches no source rule.
- **Writer.** `reviews` in `src/spitdag/write.rs`, version 14.

## spit-bash

- **Load.** `schema.py` reads `reviews`; `VERSION = 14`.
- **Plan.** After the job listing, one block per review that withholds
  anything:

  ```text
  review looked_at, qc/looked_at.csv: 1 rejected, 2 held; 3 outputs withheld
    to inspect: out/mask-03 (ready), out/mask-04 (job 4 makes it)
  ```

  A missing file says so, as does one with no verdicts, and a review the
  recipe names no file for says to add a `review ... from` line.
- **Run.** At the end, after the summary line: the held artifacts ready to
  inspect with their paths and the review's file, and those that could not
  be prepared because their job failed or was not run. Current artifacts are
  listed even when no job ran. Withheld work is not a failure and leaves the
  exit status alone.
- **`plan --json`.** Gains `reviews`, as the DAG gives them, with each
  target's readiness.

## Tests

- SPIT: `tests/reviews.rs` (or the existing review tests) for the DAG
  field: missing file, header only, mixed verdicts, a port review, a
  cascade through two steps, an aggregate that shrinks; a `.spitout`
  round trip; stored outputs re-saved.
- spit-bash: `test_reviews.py` for plan and run text, readiness, a failed
  producer, and a saved DAG whose CSV changes after it was made.
- An example under `examples/runnable` with a review, in `coverage.json`.

## Documentation

- `docs/spitdag.md`: version 14, a "Reviews" section, and the paragraph
  under Removal that says a review leaves no field.
- `docs/language-reference.md`: "What a verdict does" says the DAG records
  them.
- `runner/README.md`: the review report.
- `docs/architecture/resolution.md`: the causes kept per withheld artifact.

## Steps

- [x] Inputs: review files in the inventory and `.spitout`; header-only note;
      review file not an unmatched source (313558a).
- [x] Resolver: targets with `withholds` and `shrinks` (313558a).
- [x] Binding and writer: `reviews`, version 14; docs; stored outputs (313558a).
- [x] spit-bash: load, plan, run, `plan --json`; README; tests.
- [x] Example `examples/runnable/review` and coverage.
- [x] Benchmark with `--old`: passed (313558a).
- [x] Tests for the review files eclnz/spit#273 lists that had none: one that
      cannot be read, an empty one and a ragged row in SPIT, and a recipe
      with each in spit-bash, which stops before any job runs.

## Decisions made while building

- **The `.spitout` records review files in `verdicts:`**, as
  `looked_at from qc/looked_at.csv` with `(missing)`, rather than in a new
  section, so no header word, hover or grammar changes.
- **A printed `.spitout` writes the file as the recipe's folder gives it**,
  since where it will be kept is unknown, as it records no root.
- **The runner lists held artifacts only**, with readiness; rejected ones are
  listed with their row, since nobody needs to inspect them.
- **`plan` writes the review lines to standard output and `run` to standard
  error**, after its summary, each beside its other reports.
- **spit-vscode needs no change**: no keyword, header, `check --json` or
  `--hovers` field changed; the `verdicts:` hover text comes from SPIT.
