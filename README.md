# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

Real datasets are irregular: a subject with a missing scan, a station with three sensors instead of two, a folder that grows every week. Hand-written shell scripts and `for` loops turn every irregularity into a special case, and a missing input usually surfaces as a cryptic failure partway through a long run rather than up front.

SPIT separates the pipeline from the data. You describe the pipeline once — its steps, and how each one's inputs and outputs relate along dimensions such as subject, run, or visit — without listing actual files. Point that pipeline at an inventory of what inputs actually exist (a file, a directory scan, or a list you supply), and SPIT works out exactly which jobs that produces, validates the whole thing before anything runs (unresolvable dimensions, unknown placeholders, colliding output paths, and more), and can report precisely which artifacts it can and can't produce and why. It then emits a plain Bash script to do the work — no daemon or runtime engine to run alongside it, just the commands you already use.

Add or remove inputs and the same pipeline definition produces the right jobs, with no edits.

## Contents

- [Try it](#try-it)
- [CLI commands and options](#cli-commands-and-options)
- [Write a pipeline](#write-a-pipeline)
- [Supply the inputs](#supply-the-inputs)
- [Inspect and generate a script](#inspect-and-generate-a-script)
- [Language reference](#language-reference)
- [More examples](#more-examples)
- [How SPIT works](#how-spit-works)
- [Documentation](#documentation)
- [Development](#development)
- [Contributing](#contributing)

## Try it

Requires a [Rust toolchain](https://www.rust-lang.org/tools/install) (stable, via `cargo`). From this repository:

```sh
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- dag examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- dag examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources --json
```

Use `cargo build` to get the `target/debug/spit` executable. With `cargo run`, the `--` separates Cargo's arguments from SPIT's arguments.

Live validation in VS Code is maintained in the separate `spit-vscode` repository.

## CLI commands and options

```text
spit <check|dag|bash|artifacts|discover> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--stage <name>] [--paths] [--strict-paths] [--json] [--stdin]
```

Choose one command per call. The pipeline file comes next; options follow it.

| Command | Result |
| --- | --- |
| `check` | Validate the pipeline and report how many jobs resolve. Without an inventory, it checks the pipeline text alone and resolves no jobs. |
| `dag` | Print the jobs, their artifact identities, and dependencies. Use `--json` for a versioned logical graph. |
| `bash` | Write a Bash script for the resolved jobs to standard output. It does not run the script. |
| `artifacts` | List every concrete artifact the inventory yields: the complete ones, then the incomplete ones with why each cannot be produced. Unlike the other commands, it does not stop at a missing, ambiguous, or too-small input or a coverage gap; see [Find incomplete artifacts](#find-incomplete-artifacts). |
| `discover` | Print an inventory of source files and discovered directory contexts under `--root`. |

| Option | Effect |
| --- | --- |
| `--sources <inventory.spit>` | Read source artifact identities from a separate file. Jobs need an inventory: this file, an inline one, or sources discovered with `--root`; `check` without any checks the pipeline alone. A separate inventory replaces an inline one, which is then skipped with a warning. Use `--sources -` to read standard input. |
| `--root <directory>` | Check source paths under this directory and use it as the discovery root. A `discover` rule uses the pipeline file's folder when `--root` is omitted. Without an inventory, sources are found or expected from their path rules and discovered bindings. |
| `--paths` | With `check`, show which path rule covers each product and validate the resulting paths. With `dag`, print a path under every artifact. |
| `--stage <name>` | With `check`, `dag`, or `bash`, keep only the jobs of one [stage](docs/language-reference.md#stages) and the stages nested in it; name a nested stage by its path, such as `preprocess/combine`. Outputs of other stages that it reads are treated as files that already exist: `bash` checks for them before the first job, and `--root` checks that they are there. |
| `--strict-paths` | Require an explicit `path product:` rule for every product, even if a default `path:` rule exists. |
| `--json` | With `dag`, print the resolved logical graph as JSON; it works with `--stage`, `--root`, and `--strict-paths`, but not `--paths`. With `check`, print diagnostics as JSON for editor use and stop, succeeding whatever they report. Each diagnostic has a `severity` of `error` or `warning`; those tied to a declaration, call, rule, command, or path include its `line`, and a `column` and `end_column` for the text it is about, such as one input of a call or one `{placeholder}`. Columns are 1-based and count UTF-16 code units, as editors do; `end_column` is one past the last character. |
| `--stdin` | Read the pipeline text from standard input instead of the pipeline file, such as an editor's unsaved buffer. The pipeline path is still used to resolve `use` imports. |

For example, `check` resolves the pipeline, while `check --root` also verifies its input files:

```sh
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources --root /path/to/data
```

### Errors and warnings

Every command first reports all the problems it can find, one error per line, before doing any work. Each names the line and the column where the text at fault starts:

```text
warning: line 2, column 8: source product `spare` is never used as an input
error: line 5, column 29: command for `clean` uses unknown placeholder `{result}`
error: line 9, column 14: unknown product `rwa`
```

Syntax errors are reported throughout the file first; the remaining checks run once every line parses. A step or rule that uses a declaration which failed is not reported again. Errors stop the command; warnings do not. Warnings flag a source product no step uses, an operation no step uses, a used operation with no `command` once the pipeline has commands, an output type variable that no input binds, a stage with no steps, a shell operator such as `|` or `>` in a command, and a `#` that ends a word, which reads like a comment but is part of the word. With an inventory, they also flag a source with no artifacts, naming the steps it leaves without jobs, any other step that resolves no jobs, paths that differ only in letter case, and an inline inventory that `--sources` replaces. A file with no steps is treated as a library of definitions, and imported definitions are never reported as unused. Jobs are resolved against the inventory only when nothing else is wrong.

## Write a pipeline

Here is the complete [text processing example](examples/commands/bash_demo.spit):

```text
source shard : Lines [group, part]
require shard count>=1 per [group]

path: {product}/{entities}.txt
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines @ drop(part)
command merge: sort -m -u -o {output} {items}
merged = merge(sorted @ vary(part))
```

`source` declares a family of input artifacts. A `shard` is identified by its `group` and `part` values. `sorted` keeps those dimensions. `merge` collects all parts of each group and produces one `merged[group=...]` artifact per group. The `@ drop(part)` contract and `@ vary(part)` call must agree. For datasets whose subject and session values are unknown in advance, `discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}` reads the observed pairs from folders and expands source artifacts over them.

`path` lines say where artifacts live. `command` lines give the exact executable and argument order. SPIT decides which artifacts belong to each job before filling their paths into a command.

## Supply the inputs

The pipeline describes what to do; an inventory describes what is present. The example uses [bash_demo.sources](examples/commands/bash_demo.sources):

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

This creates two sort jobs for `alpha`, one for `beta`, and one merge job for each group. Add another shard to the inventory and SPIT creates the corresponding job without changing the pipeline.

An inventory can also be placed in the same `.spit` file for a small example, as in [basic.spit](examples/basic/basic.spit). For reusable pipelines, keep it separate and pass `--sources inventory.spit`. Use `--sources -` to read an inventory from standard input.

## Inspect and generate a script

```sh
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources --paths
cargo run -- dag examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources --paths
cargo run -- bash examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources > run.sh
SPIT_ROOT=/path/to/data bash run.sh
```

`dag --paths` shows paths before command expansion. The generated script uses `SPIT_ROOT` for relative paths; when unset, it uses the current directory.

### Find incomplete artifacts

Every other command stops at the first job the inventory cannot complete. `artifacts` resolves every job it can and reports the rest:

```sh
cargo run -- artifacts pipeline.spit --sources inventory.spit
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

An incomplete artifact has a missing or ambiguous input, a collection below its `@ min(count)`, or an input that is itself incomplete, so a gap early in the pipeline is traced through every step that depends on it. A group that fails a `require` rule is listed under `Coverage gaps`, and its sources are held back from every job. A step creates jobs only for the artifacts that drive it, so a context with no driving artifact at all appears only through the coverage gaps and steps that notice it missing. The command succeeds whatever it finds; the complete artifacts are the ones the pipeline could produce from this inventory today.

## Language reference

Beyond the basics above, `.spit` files support typed products, multi-output operations, `many`/aggregation inputs with selectors (`where`, `same`, `vary`, `each`), coverage constraints (`require`, `contexts`), symbolic type variables, stages, path placeholders, and `use` imports for sharing definitions across files. See the [full language reference](docs/language-reference.md) for syntax and rules for each of these.

## More examples

| Example | Shows |
| --- | --- |
| [Basic](examples/basic/basic.spit) | Sectioned syntax and an inventory in one file |
| [Untyped](examples/types/untyped.spit) | Resolution without types |
| [Typed](examples/types/typed.spit) | Parameterized symbolic types |
| [Branching](examples/pipelines/branching.spit) | Shared inputs and branches |
| [Complex](examples/pipelines/complex.spit) | Nested aggregation |
| [Selectors](examples/pipelines/selectors.spit) | `where`, `same`, a two-output step, a verification, and a many input beside a single input |
| [Analytics](examples/analytics/analytics.spit) | Joins and rollups |
| [Field survey](examples/commands/field_survey.spit) | A larger pipeline with sidecar files, calibration, alignment between spaces, and commands |
| [MRtrix3 ACT](examples/commands/mrtrix3_act.spit) | A larger pipeline with commands in nested preprocessing, anatomy, and tractography stages, with a folder per stage and per-stage file formats |
| [Stages](examples/stages/stages.spit) | Preprocessing and analysis stages, a stage's own path default, and `{stage}` paths |
| [Nested stages](examples/stages/nested.spit) | Stages within a stage, beside a step in the outer stage itself |
| [Imports](examples/imports/imported.spit) | Reuse source and operation definitions with `text::` names |
| [Compiler stress pipelines](examples/stress/README.md) | Deep type inference, deliberate type errors, uneven joins, and large multilevel DAGs |

Run `cargo test --test source_files` to see the field survey example checked against a temporary tree of empty source files: it resolves when every file is present, and reports a missing file, a photo without its sidecar, and a source path that is a directory. The MRtrix example imports each DWI's `.bvec`, `.bval`, and JSON metadata into a `.mif` before processing.

## How SPIT works

```text
pipeline text + source inventory
              ↓
       resolved logical DAG
              ↓
       paths and commands
              ↓
          Bash script
```

The inventory supplies artifact identities; the pipeline supplies operations and rules. Resolution checks dimensions, matching, cardinality, constraints, and any known types. Path binding and command expansion happen afterward. SPIT does not inspect file contents or command-specific metadata itself; `verify` commands run those checks with your own tools.

## Documentation

- [Language reference](docs/language-reference.md) — full `.spit` syntax
- [Architecture](docs/architecture.md) — the internal model: resolution, typing, and the Bash backend
- [Examples](docs/examples.md) — how to run each example pipeline, and what the larger ones show

## Development

```sh
cargo test
```

Runs the full test suite, including the integration tests under `tests/` that check the example pipelines end to end.

## Contributing

Issues and pull requests are welcome. For a change to the language or resolver, add or update a test under `tests/` and, if it changes behavior described here, update this README or the [language reference](docs/language-reference.md) alongside it.

## Disclaimer

SPIT was developed with the help of generative AI tools. I am not a Rust developer: most of the code and documentation was generated by AI, and I have checked it by testing its behavior rather than by expert review of the Rust itself. The test suite covers the examples and language features, but the code may still contain errors or unidiomatic Rust. SPIT generates Bash scripts that run commands on your system: review a generated script before running it, especially on data you cannot easily replace. The software is provided as is, without warranty of any kind; see the [license](LICENSE).

## License

SPIT is released under the [MIT License](LICENSE).
