# SPIT — Simple Pipeline in Text

SPIT lets you write a pipeline as a text file, check which jobs it would create, and generate a Bash script to run them. The same pipeline works with any number of observed inputs.

## Try it

From this repository:

```sh
cargo run -- check examples/bash_demo.spit --sources examples/bash_demo.sources
cargo run -- dag examples/bash_demo.spit --sources examples/bash_demo.sources
```

`check` reports whether the pipeline resolves. `dag` prints the jobs, their inputs and outputs, and their dependencies. Use `cargo build` to get the `target/debug/spit` executable.

## Write a pipeline

Here is the complete [text processing example](examples/bash_demo.spit):

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

The pipeline describes what to do; an inventory describes what is present. The example uses [bash_demo.sources](examples/bash_demo.sources):

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

This creates two sort jobs for `alpha`, one for `beta`, and one merge job for each group. Add another shard to the inventory and SPIT creates the corresponding job without changing the pipeline.

An inventory can also be placed in the same `.spit` file for a small example, as in [basic.spit](examples/basic.spit). For reusable pipelines, keep it separate and pass `--sources inventory.spit`. Use `--sources -` to read an inventory from standard input.

## Inspect and run

```sh
cargo run -- paths examples/bash_demo.spit --sources examples/bash_demo.sources
cargo run -- bound-dag examples/bash_demo.spit --sources examples/bash_demo.sources
cargo run -- check examples/bash_demo.spit --sources examples/bash_demo.sources --root /path/to/data
cargo run -- bash examples/bash_demo.spit --sources examples/bash_demo.sources > run.sh
SPIT_ROOT=/path/to/data bash run.sh
```

`paths` shows the path rule for each product. `bound-dag` shows every resolved artifact with its path, before any command is expanded. `check --root` verifies that every source file needed by the resolved jobs exists; derived outputs do not need to exist yet. `bash` writes a script that runs the jobs. `SPIT_ROOT` defaults to the current directory.

Add `--strict-paths` to require a separate `path product:` rule for every product. Without it, a `path:` default can cover several products.

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

To call a shell function, declare its script and then use the function name in a command:

```text
shell-source: scripts/functions.sh
command process: my_process {image} {output}
```

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

## More examples

| Example | Shows |
| --- | --- |
| [Basic](examples/basic.spit) | Sectioned syntax and an inventory in one file |
| [Untyped](examples/untyped.spit) | Resolution without types |
| [Typed](examples/typed.spit) | Parameterized symbolic types |
| [Branching](examples/branching.spit) | Shared inputs and branches |
| [Complex](examples/complex.spit) | Nested aggregation |
| [Analytics](examples/analytics.spit) | Joins and rollups |
| [MRtrix3 ACT](examples/mrtrix3_act.spit) | A larger pipeline with commands and paths |

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
