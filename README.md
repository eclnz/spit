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
spit <check|dag|bound-dag|paths|bash|diagnose> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--strict-paths]
```

Choose one command per call. The pipeline file comes next; options follow it.

| Command | Result |
| --- | --- |
| `check` | Validate the pipeline and report how many jobs resolve. |
| `dag` | Print the jobs, their artifact identities, and dependencies. |
| `bound-dag` | Print the resolved DAG with a path for every artifact. |
| `paths` | Show which path rule covers each product and validate the resulting paths. |
| `bash` | Write a Bash script for the resolved jobs to standard output. It does not run the script. |
| `diagnose` | Read the pipeline from standard input and return JSON diagnostics for editor use. Errors tied to a declaration or call include its source line. A pipeline path is required for CLI consistency, but its file contents are not read. |

| Option | Effect |
| --- | --- |
| `--sources <inventory.spit>` | Read source artifact identities from a separate file. Required unless the pipeline contains an inline inventory. Use `--sources -` to read standard input. |
| `--root <directory>` | Check that every required source path points to a regular file under this directory. Available with any command; derived outputs need not exist yet. |
| `--strict-paths` | Require an explicit `path product:` rule for every product, even if a default `path:` rule exists. |

For example, `check` resolves the pipeline, while `check --root` also verifies its input files:

```sh
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- check examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources --root /path/to/data
```

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
cargo run -- paths examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- bound-dag examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources
cargo run -- bash examples/commands/bash_demo.spit --sources examples/commands/bash_demo.sources > run.sh
SPIT_ROOT=/path/to/data bash run.sh
```

`bound-dag` shows paths before command expansion. The generated script uses `SPIT_ROOT` for relative paths; when unset, it uses the current directory.

## Syntax reference

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

The first input determines a normal operation's output dimensions. For an aggregation, `vary(run)` removes `run` from the output identity. You can write the output type and dimensions explicitly when helpful:

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

Declare an operation before its first use. Inputs in a call follow the port order in the declaration. A `one` input must resolve to exactly one artifact for each job; SPIT reports missing or ambiguous matches. A `many` input needs `@ vary(dimension)`, and its command placeholder expands to one separately quoted argument per artifact, ordered by entity bindings. A many placeholder must occupy a whole argument.

Input port names are optional. An unnamed single input is `{input}`; multiple unnamed inputs are `{input1}`, `{input2}`, and so on. `{output}` is the output path. Command templates give ordered words and arguments, not shell pipelines or redirection.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {output}
```

### Reuse definitions

Import named operations or source families from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path and coverage rules. Imports do not bring pipeline steps or inventory records.

```text
use shard, sort_lines from lib/text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives the imported names a prefix. Without it, the names stay unqualified: `use sort_lines from lib/text.spit` makes `sort_lines(...)` available. A source imported as `text::shard` also uses that name in `sources:` or a separate inventory. SPIT reports missing names, import cycles, and name collisions.

### Paths

```text
path: results/{product}/{entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default. `path image:` overrides it for `image`. Templates can use `{product}`, `{entities}`, or a declared dimension. Paths are relative to `SPIT_ROOT`. SPIT checks missing rules, invalid placeholders, and collisions between resolved artifact paths.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

### Constraints and optional types

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
```

Constraints check each observed group. They do not set a total subject or visit count. An inventory may include `contexts:` to name a group even when one of its required inputs is absent:

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
| [Analytics](examples/analytics/analytics.spit) | Joins and rollups |
| [MRtrix3 ACT](examples/commands/mrtrix3_act.spit) | A larger pipeline with commands and paths |
| [Imports](examples/imports/imported.spit) | Reuse source and operation definitions with `text::` names |
| [Compiler stress pipelines](examples/stress/README.md) | Deep type inference, deliberate type errors, uneven joins, and large multilevel DAGs |

SPIT also accepts grouped `products:`, `operations:`, `pipeline:`, and `constraints:` sections. The flow style above is intended for writing a pipeline in the order you read it.

Run `cargo test --test source_files` to see the MRtrix example checked against a temporary tree of empty BIDS-named NIfTI images and sidecars. The pipeline imports each DWI's `.bvec`, `.bval`, and JSON metadata into a `.mif` before processing.

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

The inventory supplies artifact identities; the pipeline supplies operations and rules. Resolution checks dimensions, matching, cardinality, constraints, and any known types. Path binding and command expansion happen afterward. SPIT currently supports one output per operation and does not inspect file contents or command-specific metadata. See [architecture](docs/architecture.md) for the internal model.

Run the test suite with `cargo test`.
