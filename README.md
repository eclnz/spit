# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

Real datasets are irregular: a subject with a missing scan, a station with three sensors instead of two, a folder that grows every week. Hand-written shell scripts and `for` loops turn every irregularity into a special case, and a missing input usually surfaces as a cryptic failure partway through a long run rather than up front.

SPIT separates the pipeline from the data. You describe the pipeline once — its steps, and how each one's inputs and outputs relate along dimensions such as subject, run, or visit — without listing actual files. Point that pipeline at an inventory of what inputs actually exist (a file, a directory scan, or a list you supply), and SPIT works out exactly which jobs that produces, validates the whole thing before anything runs (unresolvable dimensions, unknown placeholders, colliding output paths, and more), and can report precisely which artifacts it can and can't produce and why. It then writes every job, with its files and its command, to a `.spitdag` that a backend can run — no daemon or runtime engine to run alongside it, just the commands you already use.

Add or remove inputs and the same pipeline definition produces the right jobs, with no edits.

## Contents

- [Try it](#try-it)
- [The three steps and their files](#the-three-steps-and-their-files)
- [CLI commands and options](#cli-commands-and-options)
- [Write a pipeline](#write-a-pipeline)
- [Supply the inputs](#supply-the-inputs)
- [Where files live](#where-files-live)
- [Resolve jobs](#resolve-jobs)
- [Language reference](#language-reference)
- [More examples](#more-examples)
- [How SPIT works](#how-spit-works)
- [Documentation](#documentation)
- [Development](#development)
- [Contributing](#contributing)

## Try it

Requires a [Rust toolchain](https://www.rust-lang.org/tools/install) (stable, via `cargo`). From this repository:

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout -o command_demo.spitdag
```

Use `cargo build` to get the `target/debug/spit` executable. With `cargo run`, the `--` separates Cargo's arguments from SPIT's arguments.

Live validation in VS Code is maintained in the separate `spit-vscode` repository.

## The three steps and their files

SPIT runs in three steps. Each is one command, and each reads the files the previous step wrote:

| Step | Command | Reads | Writes |
| --- | --- | --- | --- |
| 1. Compile | `spit check` | a `.spit` pipeline, or a `.spitin` recipe | nothing: it reports errors and warnings |
| 2. Build inputs | `spit inputs` | a `.spitin` recipe, its pipeline, and the dataset folder | a `.spitout` |
| 3. Resolve jobs | `spit dag`, `spit artifacts` | a `.spit` pipeline and a `.spitout` | a `.spitdag` |

| File | Holds |
| --- | --- |
| `.spit` | A pipeline: sources, operations, steps, commands, and path rules. No dataset appears in it. |
| `.spitin` | A recipe for a dataset's inputs: the pipeline it serves, and its `discover`, `exclude`, `drop` and `require` rules and source paths. |
| `.spitout` | A dataset's settled inputs: each source artifact, with its file. |
| `.spitdag` | The resolved jobs, each with its artifacts' files and its command, as JSON, in an order they can run in: all a backend needs to run them, with the dataset folder, the programs the commands need, and a fingerprint of each job's work to tell when it must run again. |

A later step may also take an earlier step's input and run that step in memory: `dag` and `artifacts` take a `.spitin` in place of the `.spitout`. A `.spitin` names its own pipeline, so it is given alone: `spit dag dataset.spitin`. Giving a `.spit` beside it is an error, so the two cannot disagree. A `.spitout` names no pipeline, so it takes one: `spit dag analysis.spit dataset.spitout`.

SPIT has no backend yet: nothing in this repository runs a `.spitdag`.

## CLI commands and options

```text
spit check <pipeline.spit | recipe.spitin> [--path-rules] [--strict-paths] [--json] [--stdin]
spit inputs <recipe.spitin> [--root <directory>] [--unmatched | -o <file>]
spit dag <recipe.spitin> [--root <directory>] [--strict-paths] [--paths] [--commands] [--partial] [--json | -o <file>]
spit dag <pipeline.spit> <inputs.spitout | -> [--root <directory>] [--strict-paths] [--paths] [--commands] [--partial] [--json | -o <file>]
spit artifacts <recipe.spitin> [--root <directory>]
spit artifacts <pipeline.spit> <inputs.spitout | -> [--root <directory>]
```

Files come first; options follow them. `spit help` lists the commands, and `spit help <command>` or `spit <command> --help` gives one command's options.

| Command | Result |
| --- | --- |
| `check` | Compile a pipeline and report every problem the text shows, reading no data. Given a recipe, check its rules against the pipeline its `pipeline` line names. |
| `inputs` | Scan the dataset folder with a recipe, apply its `exclude` and `drop` rules, check its `require` rules, and print the `.spitout` with a record of what was removed. It writes nothing if a `require` rule fails. |
| `dag` | Resolve the jobs, and print each with its artifacts and dependencies. With `-o`, write them as a `.spitdag`. |
| `artifacts` | List every concrete artifact the inputs yield: the complete ones, then the incomplete ones with why each cannot be produced. Unlike `dag`, it does not stop at a missing, ambiguous, or too-small input or a coverage gap; see [Find incomplete artifacts](#find-incomplete-artifacts). |

| Option | Effect |
| --- | --- |
| `-o <file>`, `--output <file>` | With `inputs`, write the `.spitout` to the file instead of standard output. With `dag`, write the `.spitdag`. |
| `--root <directory>` | The dataset root: the folder a recipe scans, the base of every path, and where each source file must exist. By default, the folder a recipe's `root` line names, else the recipe's folder, or the root a `.spitout` records; see [Where files live](#where-files-live). |
| `--path-rules` | With `check`, list the path rule each product uses, with any [extension](docs/language-reference.md#extensions) added to it and where that is declared. |
| `--unmatched` | With `inputs`, list files under the dataset root that match no source path rule, one per line, instead of writing a `.spitout`. |
| `--paths` | With `dag`, print the file under every artifact. |
| `--commands` | With `dag`, print each job's `verify` and command lines with their paths filled in, quoted as a shell reads them, so a line can be pasted into a shell run from the dataset folder. With `--paths`, print them under each job's artifacts. Use it separately from `-o`, which saves a `.spitdag`. |
| `--partial` | With `dag`, plan jobs whose inputs can be completed and record the artifacts left out of the `.spitdag`. A `many` input uses its complete members. Without it, `dag` stops at an incomplete job. |
| `--strict-paths` | With `check` and `dag`, require an explicit `path product:` rule for every product, even if a default `path:` rule exists. Source rules settled from a recipe count when building a DAG. |
| `--json` | With `dag`, print the `.spitdag`. With `check`, print diagnostics as JSON for editor use and stop, succeeding whatever they report. Each diagnostic has a `severity` of `error` or `warning`; those tied to a declaration, call, rule, command, or path include its `line`, and a `column` and `end_column` for the text it is about, such as one input of a call or one `{placeholder}`. Columns are 1-based and count UTF-16 code units, as editors do; `end_column` is one past the last character. When checking a recipe finds an error in its pipeline, the diagnostic includes `file` and positions in that pipeline. For a pipeline that checks clean, a `paths` list gives each product whose path no rule writes in full, with its `line` and its `path`, extension included, for the editor to show. |
| `--stdin` | With `check`, read the file's text from standard input, such as an editor's unsaved buffer. The file's path is still used to resolve `use` imports and a recipe's `pipeline` line. |

Pass `-` in place of the `.spitout` to read it from standard input.

### Errors and warnings

Every command first reports all the problems it can find, one per line, before doing any work. Each names the line and the column where the text at fault starts:

```text
warning: line 2, column 8: source product `spare` is never used as an input
error: line 5, column 29: command for `clean` uses unknown placeholder `{result}`
error: line 9, column 14: unknown product `rwa`
```

Syntax errors are reported throughout the file first; the remaining checks run once every line parses. A step or rule that uses a declaration which failed is not reported again. Errors stop the command; warnings do not. Warnings flag a source product no step uses, an operation no step uses, a used operation with no `command` once the pipeline has commands, an output type variable that no input binds, a stage with no steps, a shell operator such as `|` or `>` in a command, and a `#` that ends a word, which reads like a comment but is part of the word. With inputs, as in `dag`, they also flag a source with no artifacts, naming the steps it leaves without jobs, any other step that resolves no jobs, and paths that differ only in letter case. A file with no steps is treated as a library of definitions, and imported definitions are never reported as unused. Jobs are resolved only when nothing else is wrong.

## Write a pipeline

Here is the complete [text processing example](examples/commands/command_demo/command_demo.spit):

```text
source shard : Lines [group, part]

path: {@product}/{@entities}.txt
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines
command merge: sort -m -u -o {output} {items}
merged = merge(sorted @ vary(part))
```

`source` declares a family of input artifacts. A `shard` is identified by its `group` and `part` values. `sorted` keeps those dimensions. `merge` collects all parts of each group and produces one `merged[group=...]` artifact per group. The call's `@ vary(part)` names the dimension it collects.

`path` lines say where artifacts live; an output with no rule goes to `out/{@product}/{@entities}`. `command` lines give the exact executable and argument order. SPIT decides which artifacts belong to each job before filling their paths into a command.

A pipeline names no dataset. Rules about what a dataset must hold, and records of what it does hold, go in the files of step 2: `spit check` rejects a `require` rule or a `sources:` record written in a `.spit`.

## Supply the inputs

A `.spitin` recipe describes how to find a dataset's inputs. Its first line names the pipeline it serves, relative to the recipe's folder. The smallest recipe is that line alone:

```text
pipeline analysis.spit
```

`spit inputs` then finds every source by its path rule: each file under the recipe's folder whose path matches a source's rule becomes one of that source's artifacts, with its dimensions read from the path. Nothing else is needed when the files say everything.

Add rules when they say more than the files do. For example, beside an `analysis.spit` that declares `source image: Image [sub, ses]`, `cohort.spitin` might read:

```text
pipeline analysis.spit

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
drop [sub] where sessions count<2
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

`discover` reads the observed subject and session pairs from folders, and the image source expands over them, so a session folder without its image is an error rather than a session that silently has none. `drop [sub] where sessions count<2` removes each subject with fewer than two sessions, reporting each on stderr. `require` instead fails such a group: `require sessions count>=2 per [sub]` names the subject with one session and its count. `sessions` remains a discovery rule name, not a product. `exclude bold[sub=02,ses=01,run=3]  # corrupted` removes one named artifact, and `exclude from qc/excluded.csv` reads such rules from a spreadsheet; the `.spitout` records what each rule removed and why. Logical source types and operations stay in the `.spit`.

`spit check cohort.spitin` checks the rules against the pipeline without reading the dataset. `spit inputs cohort.spitin -o cohort.spitout` scans the recipe's folder, or `--root`, and writes what it found:

```text
root .

source_paths:
    image: data/sub-{sub}/ses-{ses}/image.nii.gz

contexts sessions:
    [sub=01,ses=01]:
        image

    [sub=01,ses=02]:
        image
```

The `.spitout` lists the source identities found in the dataset, and the folder it found them in, relative to the `.spitout` itself. Paths come from the pipeline's source rules; when a recipe defines a source rule instead, the `.spitout` carries that rule once in `source_paths:`. Later steps need neither the recipe nor a rescan. A dataset indexer or person can write the same inventory. A record names no file of its own: its source's path rule gives it. The text processing example uses [command_demo.spitout](examples/commands/command_demo/command_demo.spitout):

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

This creates two sort jobs for `alpha`, one for `beta`, and one merge job for each group. Add another shard and SPIT creates the corresponding job without changing the pipeline.

## Where files live

Every path SPIT reads or writes, for a source or an output, is relative to one folder: the dataset root. The root is:

- **with a recipe:** the folder its `root` line names, else the recipe's folder;
- **with a `.spitout`:** the root its `root` line records. A printed or hand-written `.spitout` has none; then `dag` does not check that source files exist, and the `.spitdag` records no root.

`--root` overrides either, for `inputs`, `dag` and `artifacts` alike: the folder a recipe scans, the base of every path, and where each source file must exist. A recipe's `pipeline` and `root` lines are relative to the recipe's own folder and may use `..`. `spit inputs -o` records the root in the `.spitout` it writes, relative to that file, so `dag` on the `.spitout` finds the same files. A printed `.spitout` records none, since where it will be kept is unknown.

Three layouts work well:

- **Everything together:** the pipeline and recipe sit in the dataset folder, and `spit dag data/cohort.spitin` needs no `--root`. The scan ignores SPIT's own files, so they are not reported as matching no source rule.
- **The data in a folder of its own:** the pipeline and recipe sit together, and the recipe says where the data is:

  ```text
  pipeline cohort.spit
  root data
  ```

  `spit dag cohort.spitin` then reads `data/`, and every path rule is written relative to it.
- **The pipeline apart:** the pipeline lives with your code, and the recipe sits in the dataset folder with `pipeline ../code/analysis.spit`.

Write each path rule relative to the root: `path image: sub-{sub}/image.nii.gz` for files at `<root>/sub-01/image.nii.gz`.

## Resolve jobs

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit --path-rules
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout --paths
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout --commands
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout -o command_demo.spitdag
```

`dag --paths` shows each artifact's file, `dag --commands` shows the exact command lines each job will run, and `-o` writes the `.spitdag`. It holds everything a backend needs to run the jobs, so a backend reads nothing else: no pipeline, path rule or command template. Paths in it are relative to the dataset folder. The [`.spitdag` reference](docs/spitdag.md) describes each field.

### Find incomplete artifacts

`dag` stops at the first job the inputs cannot complete. `artifacts` resolves every job it can and reports the rest:

```sh
cargo run -- artifacts pipeline.spit inputs.spitout
```

```text
Complete artifacts: 10
  scan[subject=01,run=1]  (source)
  ...
  merged[subject=01] : Scan  (job 6: merge)

Incomplete artifacts: 2
  aligned[subject=02,run=1] : Scan  (align)
    - no `calibration` artifact for input `reference` of `align` at [run=1,subject=02]
  merged[subject=02] : Scan  (merge)
    - input `runs` needs aligned[subject=02,run=1], which cannot be produced
```

An incomplete artifact has a missing or ambiguous input, a collection below its `@ min(count)`, or an input that is itself incomplete, so a gap early in the pipeline is traced through every step that depends on it. Given a recipe, a group that fails a `require` rule is listed under `Coverage gaps`, and its sources are held back from every job. A step creates jobs only for the artifacts that drive it, so a context with no driving artifact at all appears only through the coverage gaps and steps that notice it missing. The command succeeds whatever it finds; the complete artifacts are the ones the pipeline could produce from these inputs today.

`artifacts` also lists, under `Unused sources`, each source that no job reads, whether or not that job can be completed. Some are left out on purpose, such as calibration revisions a `where(revision=3)` selector passes over; others point to a mistake, such as `pricing/S07.json` read as store `S07` where the pipeline needs `s07`. `dag` counts them in a note: `3 source artifacts are used by no job (calibration: 3)`.

When a missing input differs from an unused source only in letter case or leading zeros, the failed `dag` and `artifacts` reports name that source and the differing dimension. They also warn that the source is unused. A genuinely missing source has no such hint. During discovery, `inputs` notes how many files match no source rule; run `spit inputs dataset.spitin --unmatched` to list them.

To make a plan for the work that can run now, use `spit dag dataset.spitin --partial -o plan.spitdag`. The command succeeds and records each unproducible output and its reasons in `left_out`. A `many` input takes only its complete members, then applies any `@ min(count)` requirement to that smaller collection. For example, a chain summary can use the reports from good stores even when other stores' reports cannot be made. Use `spit artifacts dataset.spitin` to inspect the full set of gaps before running the plan.

## Language reference

Beyond the basics above, `.spit` files support typed products, multi-output operations, `many`/aggregation inputs with selectors (`where`, `same`, `vary`, `each`), symbolic type variables, stages, path placeholders, and `use` imports for sharing definitions across files; `.spitin` recipes add directory discovery, rules that leave data out (`exclude`, `drop`) or require it (`require`). See the [full language reference](docs/language-reference.md) for syntax and rules for each of these.

## More examples

| Example | Shows |
| --- | --- |
| [Basic](examples/basic/basic.spit) | A small pipeline, with its recipe and inputs in separate files |
| [Untyped](examples/types/untyped.spit) | Resolution without types |
| [Typed](examples/types/typed.spit) | Parameterized symbolic types |
| [Branching](examples/pipelines/branching.spit) | Shared inputs and branches |
| [Complex](examples/pipelines/complex.spit) | Nested aggregation |
| [Selectors](examples/pipelines/selectors.spit) | `where`, `same`, a two-output step, a verification, and a many input beside a single input |
| [Analytics](examples/analytics/analytics.spit) | Joins and rollups |
| [Field survey](examples/commands/field_survey/field_survey.spit) | A larger pipeline with sidecar files, calibration, alignment between spaces, and commands |
| [MRtrix3 ACT](examples/commands/mrtrix3_act/mrtrix3_act.spit) | A larger pipeline with commands in nested preprocessing, anatomy, and tractography stages, with a folder per stage and per-stage file formats |
| [Stages](examples/stages/stages.spit) | Preprocessing and analysis stages, a stage's own path default, and `{@stage}` paths |
| [Nested stages](examples/stages/nested.spit) | Stages within a stage, beside a step in the outer stage itself |
| [Imports](examples/imports/imported.spit) | Reuse source and operation definitions with `text::` names |
| [Compiler stress pipelines](examples/stress/README.md) | Deep type inference, deliberate type errors, uneven joins, and large multilevel DAGs |

Run `cargo test --test source_files` to see the field survey example checked against a temporary tree of empty source files: it resolves when every file is present, and reports a missing file, a photo without its sidecar, and a source path that is a directory. The MRtrix example imports each DWI's `.bvec`, `.bval`, and JSON metadata into a `.mif` before processing.

## How SPIT works

```text
.spit ──► 1. check ──► compiled pipeline
                            │
.spitin + data ──► 2. inputs ──► .spitout
                            │        │
                            ▼        ▼
                        3. dag ──► .spitdag
```

The pipeline supplies operations and rules; the `.spitout` supplies artifact identities and their files. Resolution checks dimensions, matching, cardinality, and any known types, then binds each artifact to its file and expands each command into its arguments. Step 2 and step 3 each build on step 1 and never on each other, and a backend would read only the `.spitdag`. SPIT does not inspect file contents or command-specific metadata itself; `verify` commands run those checks with your own tools.

## Documentation

- [Language reference](docs/language-reference.md) — full `.spit`, `.spitin` and `.spitout` syntax
- [The `.spitdag` format](docs/spitdag.md) — every field a backend reads
- [Architecture](docs/architecture.md) — the internal model: resolution, typing, and the bound DAG
- [Examples](docs/examples.md) — complete walkthroughs for sweeps, cohorts, selectors, and stages, plus the larger pipeline catalog

## Development

```sh
cargo test
```

Runs the full test suite, including the integration tests under `tests/` that check the example pipelines end to end.

## Contributing

Issues and pull requests are welcome. For a change to the language or resolver, add or update a test under `tests/` and, if it changes behavior described here, update this README or the [language reference](docs/language-reference.md) alongside it.

## Disclaimer

SPIT was developed with the help of generative AI tools. I am not a Rust developer: most of the code and documentation was generated by AI, and I have checked it by testing its behavior rather than by expert review of the Rust itself. The test suite covers the examples and language features, but the code may still contain errors or unidiomatic Rust. The commands in a `.spitdag` run on your system when a backend runs them: review them before running them, especially on data you cannot easily replace. The software is provided as is, without warranty of any kind; see the [license](LICENSE).

## License

SPIT is released under the [MIT License](LICENSE).
