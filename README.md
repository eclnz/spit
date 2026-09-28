# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

SPIT lets you write a pipeline as a text file, check which jobs it would create, and generate a Bash script to run them. The same pipeline works with any number of observed inputs.

## Try it

From this repository:

```sh
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- dag examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
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
| `dag` | Print the jobs, their artifact identities, and dependencies. |
| `bash` | Write a Bash script for the resolved jobs to standard output. It does not run the script. |
| `artifacts` | List every concrete artifact the inventory yields: the complete ones, then the incomplete ones with why each cannot be produced. Unlike the other commands, it does not stop at a missing, ambiguous, or too-small input or a coverage gap; see [Find incomplete artifacts](#find-incomplete-artifacts). |
| `discover` | Print an inventory of the source files under `--root`, found by matching each file against the sources' path rules. |

| Option | Effect |
| --- | --- |
| `--sources <inventory.spit>` | Read source artifact identities from a separate file. Jobs need an inventory: this file, an inline one, or sources discovered with `--root`; `check` without any checks the pipeline alone. A separate inventory replaces an inline one, which is then skipped with a warning. Use `--sources -` to read standard input. |
| `--root <directory>` | Check that every required source path points to a regular file under this directory; derived outputs need not exist yet. Without `--sources` or an inline inventory, the sources are discovered under this directory from their path rules. |
| `--paths` | With `check`, show which path rule covers each product and validate the resulting paths. With `dag`, print a path under every artifact. |
| `--stage <name>` | With `check`, `dag`, or `bash`, keep only the jobs of one [stage](#stages) and the stages nested in it; name a nested stage by its path, such as `preprocess/combine`. Outputs of other stages that it reads are treated as files that already exist: `bash` checks for them before the first job, and `--root` checks that they are there. |
| `--strict-paths` | Require an explicit `path product:` rule for every product, even if a default `path:` rule exists. |
| `--json` | With `check`, print the diagnostics as JSON for editor use and stop, succeeding whatever they report. Each has a `severity` of `error` or `warning`; those tied to a declaration, call, rule, command, or path include its `line`, and a `column` and `end_column` for the text it is about, such as one input of a call or one `{placeholder}`. Columns are 1-based and count UTF-16 code units, as editors do; `end_column` is one past the last character. |
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

Syntax errors are reported throughout the file first; the remaining checks run once every line parses. A step or rule that uses a declaration which failed is not reported again. Errors stop the command; warnings do not. Warnings flag a source product no step uses, an operation no step uses, a used operation with no `command` once the pipeline has commands, an output type variable that no input binds, a stage with no steps, and a `#` that ends a word, which reads like a comment but is part of the word. With an inventory, they also flag a source with no artifacts, naming the steps it leaves without jobs, any other step that resolves no jobs, and an inline inventory that `--sources` replaces. A file with no steps is treated as a library of definitions, and imported definitions are never reported as unused. Jobs are resolved against the inventory only when nothing else is wrong.

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

`source` declares a family of input artifacts. A `shard` is identified by its `group` and `part` values. `sorted` keeps those dimensions. `merge` collects all parts of each group and produces one `merged[group=...]` artifact per group. The `@ drop(part)` contract and `@ vary(part)` call must agree.

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

## Syntax reference

A `#` that starts a word begins a comment, as in Bash. A `#` inside a word or in quotes is kept, so `--color=#fff` and `'#run'` are ordinary arguments.

### Products and dimensions

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
```

Each `source` declares a product family, not an individual file. `image[subject=A,visit=1,run=2]` identifies one artifact. Types such as `Image` are optional; product names and entity bindings identify artifacts.

An assignment introduces a derived product automatically:

```text
processed = process(image)
average = mean(processed @ vary(run))
```

The input with the most dimensions drives a normal operation and gives its outputs their dimensions, wherever it sits among the ports. For an aggregation, `vary(run)` removes `run` from the output identity. You can write the output type and dimensions explicitly when helpful:

```text
average : Image [subject, visit] = mean(processed @ vary(run))
```

### Operations and commands

```text
operation process(image: Image) -> Image
command process: process_tool --in {image} --out {output}

operation mean(images: many Image) -> Image @ drop(run)
command mean: mean_tool {images} --out {output}
```

Declare an operation before its first use. Inputs in a call follow the port order in the declaration. A `one` input must resolve to exactly one artifact for each job, so every other input may only use dimensions the driving input has; SPIT rejects a pipeline that breaks this before reading any inventory, and reports a missing match for a job. A `many` input needs `@ vary(dimension)`, and its command placeholder expands to one separately quoted argument per artifact, ordered by the product's dimensions with numbers compared as numbers, so `run=2` comes before `run=10`. A many placeholder must occupy a whole argument. An operation takes at most one `many` input, which may sit beside `one` inputs; each of those is matched once per group:

```text
operation summarise(days: many Series, policy: Policy) -> Summary @ drop(day) @ min(2)
summary = summarise(reading @ vary(day), policy)
```

`@ min(2)` rejects a group with fewer than two artifacts.

Selectors narrow what an input matches:

```text
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
```

`where(revision=2)` keeps the artifacts with that value and takes `revision` out of matching, so a family with an extra dimension can join a less specific input. `same(station)` matches on `station` alone; the reference's other dimensions must then leave exactly one artifact for each job. Selectors can be combined, as in `frame @ where(acq=fast) @ vary(run)`.

An operation can write several outputs in one job. Name each output; its name is its placeholder, and the call assigns one product to each:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

A `verify` command checks a job's inputs before its command runs, using the tools that understand the files; if it fails, the script stops:

```text
verify register: check_same_grid {moving} {reference}
```

Input port names are optional. An unnamed single input is `{input}`; multiple unnamed inputs are `{input1}`, `{input2}`, and so on. An operation whose only input is a `many` input can also reach it as `{inputs}`, whatever its name. Named ports give clearer errors, although errors also name the product bound to a port. `{output}` is the path of a single unnamed output, so `output` cannot name an input port. A command must use every output placeholder; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. Write `{{` or `}}` for a literal brace. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and `{output}` must appear.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {output}
```

### Stages

A stage groups the steps of one phase of a pipeline, such as preprocessing or analysis. Write `stage name:` at the start of a line and indent the stage's lines beneath it; the next line that is not indented ends the stage. From the [stages example](examples/stages/stages.spit):

```text
path: {stage}/{product}/{entities}.txt

source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

stage preprocess:
    operation sort_lines(input: Lines) -> Lines
    command sort_lines: sort -u -o {output} {input}
    sorted = sort_lines(shard)

    operation merge(items: many Lines) -> Lines @ drop(part)
    command merge: sort -m -u -o {output} {items}
    merged = merge(sorted @ vary(part))

stage analysis:
    path: results/{product}/{entities}.txt

    operation tally_lines(input: Lines) -> Tally
    command tally_lines: uniq -c {input} {output}
    tally = tally_lines(merged)
```

A stage owns the products its steps assign. Operations and commands stay global, so one declared in a stage can be used anywhere, and product names are not prefixed: `analysis` reads `merged` by name. Sources, `require` rules, and `use` lines belong at the top level. A `path:` line inside a stage is the default for that stage's products only; a `path product:` rule still takes precedence. `{stage}` in a path template is the name of the product's stage.

Stages nest. A `stage` header inside a stage opens a stage within it, named by its path, such as `preprocess/combine`; a line back at the outer stage's indentation closes it. From the [nested example](examples/stages/nested.spit):

```text
stage preprocess:
    stage clean:
        sorted = sort_lines(shard)

    stage combine:
        merged = merge(sorted @ vary(part))

    resorted = sort_lines(merged)    # in `preprocess` itself
```

The lines directly in a stage share one indentation. A nested stage without its own `path:` line uses the nearest one around it, and `{stage}` gives one directory per level, as in `preprocess/combine/merged/...`.

SPIT orders stages by the products they read, so a stage needs no `after` clause. Stages must not depend on each other in a cycle, even through steps outside every stage. A nested stage is compared with its siblings, and counts toward its outer stage's place among the outer stage's siblings; a step written in an outer stage itself, like one outside every stage, passes on what it reads. `check` counts the jobs in each outermost stage, `dag` names each job's stage, and `bash` marks where each stage starts. To run one stage, such as the analysis after preprocessing has already run, pass `--stage`; a stage includes the stages nested in it, and `--stage preprocess/combine` names a nested one:

```sh
cargo run -- bash examples/stages/stages.spit --sources examples/stages/stages.sources --stage analysis
```

Stages are written in the flow form; a sectioned document cannot declare them. A step outside every stage stays valid.

### Reuse definitions

Import operations and source families from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path and coverage rules. Imports do not bring pipeline steps or inventory records.

```text
use text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives every imported name a prefix. Without it, `use text.spit` brings the names into the current scope. To import only a few definitions, use `use shard, sort_lines from text.spit as text`. A source imported as `text::shard` also uses that name in `sources:` or a separate inventory. SPIT reports missing names, import cycles, and name collisions.

### Paths

```text
path: results/{product}/{entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default. `path image:` overrides it for `image`. Each output of a multi-output step has its own product, so its own rule. A `path:` line inside a [stage](#stages) sets the default for that stage's products. Paths are relative to `SPIT_ROOT`.

A template fills these placeholders from the artifact it names, here `aligned[subject=A,run=2]` made in stage `preprocess/align`:

| Placeholder | Expands to | Example |
| --- | --- | --- |
| `{product}` | The product's name; an imported `alias::name` becomes `alias.name` | `aligned` |
| `{entities}` | Every dimension as `dim=value`, in declared order, joined by `__`; `global` for a product with no dimensions | `subject=A__run=2` |
| `{stage}` | The stage whose block holds the step, one directory per level; an error for a product made outside every stage | `preprocess/align` |
| `{subject}`, `{run}`, … | The value of a dimension the product declares | `A`, `2` |

`product`, `entities`, and `stage` are reserved: no product may declare a dimension with one of those names. Values keep letters, digits, and `-`; any other byte is written as `%` and two hex digits, so a value never adds a directory.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare, a rule that omits one of the product's dimensions (use `{entities}` or name each one), and two products whose rules give the same path for the same entities, such as a default rule without `{product}`. Missing rules are reported by `--paths`, `bash`, and `--root`, and collisions between resolved artifact paths once jobs are bound.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

Path rules also find sources. `spit discover pipeline.spit --root data` lists each file under `data` whose path matches a source's rule, reading entity values from its placeholders, as inventory text. Other commands given `--root` and no inventory do the same, so `spit bash pipeline.spit --root data` needs no inventory file.

### Constraints and optional types

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
```

Constraints check each observed group. They do not set a total subject or visit count. A rule can also require particular values in each group, alone or with a count:

```text
require image run=1,2 per [subject, visit]
```
 An inventory may include `contexts:` to name a group even when one of its required inputs is absent:

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]
```

Types are additive. You can leave them out, add them to selected products and operations, or type the whole pipeline. Known mismatches fail; missing type information does not.

In operation signatures, a single capital letter such as `S` is a local type variable. Use a `$` prefix for longer names, such as `$SourceSpace` or `$Kind`. An unprefixed name such as `World` is a concrete type. Variables are allowed in operation signatures, not product declarations:

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```

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
| [SPIT in SPIT](examples/self_host/spit_in_spit.spit) | SPIT's own format, build, test, and lint steps as a pipeline, run for real against the crate |
| [Compiler stress pipelines](examples/stress/README.md) | Deep type inference, deliberate type errors, uneven joins, and large multilevel DAGs |

SPIT also accepts grouped `products:`, `operations:`, `pipeline:`, and `constraints:` sections. The flow style above is intended for writing a pipeline in the order you read it.

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

The inventory supplies artifact identities; the pipeline supplies operations and rules. Resolution checks dimensions, matching, cardinality, constraints, and any known types. Path binding and command expansion happen afterward. SPIT does not inspect file contents or command-specific metadata itself; `verify` commands run those checks with your own tools. See [architecture](docs/architecture.md) for the internal model.

Run the test suite with `cargo test`.
