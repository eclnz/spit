# SPIT User Manual Coverage Outline

A reference outline for writing pipelines, inspecting plans, and running jobs.

## First run

The sort-and-merge example takes three text files and produces one sorted list of unique lines per group.

### Installation

Use a stable Rust toolchain and Python 3.10 or newer on Linux or macOS. From the root of a [SPIT checkout](https://github.com/eclnz/spit/tree/dev), build the compiler and install the runner in a virtual environment:

```sh
cargo build --release
export PATH="$PWD/target/release:$PATH"
python3 -m venv .venv
. .venv/bin/activate
python -m pip install ./runner
spit --version
spit-bash --help
```

### Example pipeline

`examples/runnable/lines/lines.spitin` selects `lines.spit` and uses its own directory as the dataset root. These lines show how the pipeline groups work:

```spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

sorted = sort_lines(shard)
merged = merge(sorted @vary(part))
```

`input/alpha/01.txt` is the `shard` artifact with `group=alpha` and `part=01`. The first step sorts each shard. The second gathers the sorted `part` files within each `group` and merges them. The output keeps `group` and drops `part`.

### Plan preview

```sh
spit dag examples/runnable/lines/lines.spitin --counts --commands
```

The plan lists three sort jobs and two merge jobs. `--commands` shows each command with its file paths filled in.

### Execution

```sh
spit-bash run examples/runnable/lines/lines.spitin -j 3
cat examples/runnable/lines/output/merged/group=alpha.txt
cat examples/runnable/lines/output/merged/group=beta.txt
```

The alpha file contains `apple`, `banana`, and `pear` on separate lines. The beta file contains `yak` and `zebra`.

[Complete example](https://github.com/eclnz/spit/tree/dev/examples/runnable/lines) · [Runnable examples](https://github.com/eclnz/spit/blob/dev/examples/runnable/README.md#sort-and-merge)

## Artifacts

### Identity

A **product** names a family, such as `shard`, `sorted`, or `merged`. An **artifact** is one member of that family. Its identity combines the product name with its dimension values.

| Identity | Origin |
| --- | --- |
| `shard[group=alpha,part=01]` | Input supplied by the dataset |
| `sorted[group=alpha,part=01]` | Output of one sort job |
| `merged[group=alpha]` | Output of one merge job |

A dimensionless source such as `source calibration` has one artifact. Every job that takes it can use that same artifact. A pipeline’s dimension order controls how identities are written and how collections are sorted. Source declarations establish the order where they can. Use a top-level `dimensions [...]` declaration when sources leave the order unspecified.

### Types

Types describe what a product or port accepts. They are optional by default. `Lines` is a concrete type. `Frame<Native>` is a parameterised type whose argument identifies a coordinate space.

```spit
type Frame<Space>
source scan : Frame<Native> [subject]
operation smooth(frame: Frame<S>) -> Frame<S>
smoothed = smooth(scan)
```

`S` binds separately for each call. Here the input binds it to `Native`, so SPIT infers `Frame<Native>` for `smoothed`. A known mismatch, such as connecting `Lines` to the `frame` port, is an error. `types strict` also rejects missing type information. Inferred types still count.

### Paths

A path rule maps an artifact identity to a file or folder. Source paths are relative to the dataset root. Generated paths use the output root when one is set.

```spit
path shard: input/{group}/{part}.txt
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
```

- `{group}` and `{part}` insert individual dimension values. `{@product}` inserts the product name.
- `{@entities}` inserts all dimensions in pipeline order. `default entities:` sets their spelling. Here `sorted[group=alpha,part=01]` maps to `output/sorted/group=alpha_part=01.txt`.
- Brackets make a path segment optional. In `output/{@product}[/{@entities}].txt`, the slash and entities are omitted for a dimensionless product.
- A product-specific rule takes precedence over a default. For outputs, the nearest stage default takes precedence over the pipeline default. `spit check --path-rules` lists the effective rules and where they came from.
- SPIT percent-escapes dimension values in paths. For example, `A_B` becomes `A%5FB`.
- If paths collide, include the product name and all dimensions needed to distinguish the artifacts.

### File kinds

- **Extensions** — Declare an extension such as `.txt` or `.nii.gz` after the type. It completes a path rule that leaves the extension out.
- **Folders** — A trailing `/` in a source or output declaration marks a folder artifact. Its path rule names the folder without a trailing slash.
- **Source sidecars** — `source metadata : Json .json beside raw` uses the `raw` source’s dimensions and path stem. A source at `sample.raw` has its sidecar at `sample.json`.
- **Companion outputs** — Use `beside` when a tool chooses a companion’s filename. A suffix replaces the sibling’s extension by default. Prefix the suffix with `+` to append it to the full name, as in `reference.fasta.fai`.

[Products and dimensions](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#products-and-dimensions) · [Optional types](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#optional-types) · [Paths](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#paths)

## Operations

### Signatures

An operation declares the inputs a step can pass and the outputs it produces.

```spit
operation sort_lines(input: Lines) -> Lines
operation merge(items: many Lines) -> Lines
```

Call arguments follow input port order. `sort_lines` takes one `Lines` artifact per job. `merge` takes a collection of `Lines` artifacts. The call picks which dimensions to gather, as in `merge(sorted @vary(part))`.

An operation that produces multiple outputs names each port, as in `-> (low: Series .csv, high: Series .csv)`. The step assigns products in port order. The command refers to the paths as `{low}` and `{high}`.

### Command templates

```spit
command sort_lines: sort -u -o {@output} {input}
```

For `shard[group=alpha,part=01]`, SPIT plans this command:

```sh
sort -u -o output/sorted/group=alpha_part=01.txt input/alpha/01.txt
```

`{input}` inserts the input path and `{@output}` the unnamed output path. `spit dag --commands` previews the resulting arguments. Give the executable’s path or put it on `PATH`. Quote literal arguments containing spaces. A substituted file path stays one argument.

| Form | Effect |
| --- | --- |
| `{image.stem}` | Output filename without its extension |
| `{image.dir}` | Output’s parent directory |
| `{group}` | Value of the job’s `group` dimension |
| `{items}` | One argument per artifact in the collection |
| `@join(items)` | Collection paths joined into one argument |
| `@list(items)` | Path to a file listing the collection paths |
| `@scalar(count, min=1)` | Number from the `count` input file, with a lower bound |
| `{@here}/scripts/sort.sh` | Script path relative to the file that declares the operation |

Use `< {input}` to read a file as standard input and `> {@output}` to capture standard output in a declared file. To run several commands in one job, put each on its own indented line beneath `command name:`. They run in order and stop at the first failure. For pipes or shell expressions, invoke a shell explicitly and pass file paths as arguments.

### Tools

A recipe uses `tools from tools.spitenv` to record the tool identities for that run. An identity can be fixed text, such as a version or digest, or the output of a `probe` command. A failed probe stops the run before jobs start.

With no `uses` settings, jobs depend on every tool in the environment. Declare a named tool with `tool sort_tool` and use `uses sort_lines: sort_tool` to limit the dependency. When that identity changes, the runner can rerun the jobs that depend on it.

Use `anchored sort_lines` only if the operation’s outputs depend on stored file paths. An anchored job reruns when those paths change.

### Resources

`resources align: cores=4, memory=8G` requests four cores and eight GiB for each `align` job. Put `{@cores}` where the executable takes its thread count. An operation without resource settings requests one core and claims no memory.

### Verification

`verify` checks a job’s inputs before its main command. Use it for a condition involving several inputs, such as whether two files have matching row counts. If verification fails, the main command is skipped.

### Artifact checks

```spit
check nonempty: test -s {@path}
operation sort_lines(input: Lines) -> Lines @check(nonempty)
```

`check` tests one artifact. In this example, the runner checks the sorted file after writing it. Attach checks to sources or input ports for validation before the main command. Parameterised checks take arguments at attachment, as in `@check(ndim(3))`. Use `default check: nonempty` to apply a check to every output in the file or stage.

[Operation syntax](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#operations-and-commands) · [Artifact checks](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#checks)

## Pipelines

### Syntax

Use descriptive names for products. Statement keywords such as `source` and `stage` are reserved. An unquoted `#` starts a comment at the beginning of a word.

`source:` opens a block of source declarations. Indent each entry and omit the repeated `source` keyword. A call or signature can span lines while its parentheses remain open.

### Steps

```spit
sorted = sort_lines(shard)
merged = merge(sorted @vary(part))
```

A step calls an operation with products as arguments. The assignment names the product it creates. Here each `merge` job depends on the `sort_lines` jobs that make the sorted shards it reads.

Declare an operation before its first call. Steps can appear in any order. If SPIT reports a cycle such as `a -> b -> a`, break one of the dependencies or introduce a separate input.

### Matching

SPIT normally uses the input with the most dimensions to drive the jobs. A reading with dimensions `[station, day]` can take a policy with dimension `[station]`. Each reading gets the policy for its station. A dimensionless input can be shared across all jobs that take it.

A port that takes one artifact needs exactly one matching artifact per job. A missing match leaves work incomplete. For an ambiguous match, inspect the candidate identities and add a selector that leaves one match.

- `calibration @where(revision=2)` selects revision 2 and removes `revision` from matching.
- `reference @same(station)` matches on `station` alone. Two references for the same station still produce an ambiguity.

### Collections

```spit
operation merge(items: many Lines @min(2)) -> Lines
merged = merge(sorted @vary(part))
```

`many` declares the port’s cardinality. `@vary(part)` gathers all parts within each group. `@min(2)` would reject a group with only one part, including beta in the first-run example. The result has one artifact per group and drops the `part` dimension.

Collections follow pipeline dimension order, with numeric runs sorted naturally, so part `2` comes before part `10`. Parallel collections must have the same dimensions, gather the same dimensions, and contain matching identities. An artifact missing from one collection makes the pair incomplete.

Combine selectors when needed. `frame @where(acq=fast) @vary(run)` filters to the fast acquisition before gathering runs.

### Broadcasting

```spit
forecast = predict(reading, model @each(scenario))
```

If there are two observed station readings and two scenario models, this plans four jobs. Each forecast gains a `scenario` dimension. Broadcasting uses observed driving artifacts, so a missing station-day pair produces no forecast. Only one input may broadcast a given dimension, and the driving input must lack that dimension.

### Composite operations

An operation can contain steps. Its body reads the input ports by name and assigns each named output. Products used inside the body stay private to the call. A body can call another composite operation. Use `spit check pipeline.spit --calls` to inspect the expanded steps.

### Stage structure

`stage name:` groups indented steps. Stages can nest. A stage can set defaults such as `default path:` for its products, and a product-specific rule takes precedence. Place sources and imports at the top level. Operation names remain global, including operations declared inside stages. Product dependencies determine execution order.

### Imports

```spit
use shard, sort_lines from text.spit as text
sorted = text::sort_lines(text::shard)
```

The import path is relative to the importing file. `as text` qualifies imported names. An imported operation carries its command or body and its required definitions. An imported source carries its path rule. The imported file’s top-level steps stay in that file. Use distinct aliases to resolve name collisions and remove circular imports.

[Matching](https://github.com/eclnz/spit/blob/dev/docs/guide/matching.md) · [Pipeline declarations](https://github.com/eclnz/spit/blob/dev/docs/guide/pipelines.md) · [Import rules](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#reuse-definitions)

## Datasets

### Recipes

A `.spitin` recipe selects a pipeline and identifies the data for one run. The sort-and-merge recipe uses the directory that contains it:

```spit
pipeline lines.spit
root .
```

Both paths are relative to the recipe file. Omit `root` in the recipe to inherit a root declared in the pipeline. Declare the root once. Use `output results` to give generated files a separate root.

Put dataset-specific source paths in the recipe’s `inputs:` block. For example, `shard: text/{group}/{part}.txt` supplies a path for `shard`. Use this when the pipeline leaves that source’s path unspecified. Defining the same source’s path in both files is an error.

### Discovery

Source paths capture dimensions from files. The rule `input/{group}/{part:digits}.txt` reads `input/alpha/01.txt` as `shard[group=alpha,part=01]`. The `digits` shape restricts the part value to digits.

An `inputs:` entry such as `contexts: input/{group}` discovers group identities from directories, even empty ones. This lets a coverage rule report zero shards for an existing group. A directory that is absent supplies no context. Use `spit inputs --unmatched` to inspect files outside the source rules.

### Exclusions

- `exclude shard[group=alpha,part=02]` excludes a specific shard.
- `exclude [group=beta]` excludes artifacts and contexts with `group=beta`.
- `exclude [group] where shard count<2` excludes groups with fewer than two shards. Conditional exclusions can also test whether dimension values are present or missing.

Files stay on disk. Add a comment to record why a rule exists. Saved inventories record what the rules removed.

### Coverage requirements

```spit
require [group] where shard count>=2
require [group] where shard has part=01
```

The first rule requires two or more shards in each retained group. The second requires part `01` in each retained group. A failed requirement stops planning. SPIT applies named exclusions first, conditional exclusions second, and requirements last, regardless of line order.

### Human review

Declare a review with `review shard_qc` in the pipeline. Attach it with `@review(shard_qc)` to a source or output port, or use `default review: shard_qc` for outputs in scope. In the recipe, `review shard_qc from qc/shards.csv` identifies the verdict file.

For shards, the CSV can have columns `group,part,verdict`. A row such as `alpha,01,pass` allows that artifact through. A `fail` verdict or a missing verdict withholds the artifact from the readers covered by the review. A job that produces an artifact for review can still run.

### Inventories

```sh
spit inputs dataset.spitin -o dataset.spitout
spit dag pipeline.spit dataset.spitout
```

A `.spitout` records the inputs for a plan. It can be generated or written by hand. `sources:` lists observed artifact identities, and `contexts:` can record groups without source files. `source_paths:` carries recipe-supplied path rules. `removed:` and `verdicts:` carry exclusions and review results.

A hand-written inventory can start with a `sources:` block containing `shard[group=alpha,part=01]`. The planner also needs a source path rule for `shard`. Give the inventory a `root` to have planning check that the source files exist on disk.

[Recipes and inventories](https://github.com/eclnz/spit/blob/dev/docs/guide/recipes.md) · [Coverage rules](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#constraints) · [Reviews](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md#reviews)

## Plans

### Pipeline checking

```sh
spit check examples/runnable/lines/lines.spit --path-rules
```

`spit check` reports syntax and type errors at their source locations. `--path-rules` lists the effective path rule for each product and where it came from. Run `spit check pipeline.spit --calls` to inspect expanded composite calls.

`spit check --json --hovers` supplies editor diagnostics and hover text such as inferred types, producers, and paths. Add `--stdin` to check unsaved text while retaining the named file’s path for relative references.

### Pipeline topology

```sh
spit dag examples/runnable/lines/lines.spit --tree
```

The tree shows product dependencies before a dataset is selected. Follow `sorted` from the sort step to the merge step. For staged pipelines, use the stage overview to find the products that connect stages.

### Job resolution

```sh
spit dag examples/runnable/lines/lines.spitin --counts --commands
```

The recipe gives SPIT the dataset. `--counts` lists jobs per step, including zero counts. In the example, three shards produce three sort jobs and two merge jobs. `--commands` shows the resolved command arguments. Use `--jobs` to list job inputs and outputs, or `--paths` for the resolved artifact paths.

### Missing work

```sh
spit inputs dataset.spitin --unmatched
spit artifacts dataset.spitin --by-target
```

Check discovery first. A source rule may match no files or capture an unexpected value. `--unmatched` lists files outside all source rules.

Then trace the missing target with `spit artifacts`. The report gives causes for incomplete artifacts and lists unused sources. `--by-target` groups incomplete artifacts under the final targets they prevent. An unused source with `group=Alpha` can explain a missing match for `group=alpha`: identity values are case-sensitive.

### Partial planning

By default, `spit dag` stops at an incomplete job. Use `spit dag dataset.spitin --partial -o plan.spitdag` to keep the work with complete inputs. The DAG records omitted outputs and their causes in `left_out`.

A collection keeps its complete members, then applies `@min(n)` to what remains. For example, a collection with one complete member still fails `@min(2)`.

### DAGs

```sh
spit dag dataset.spitin -o plan.spitdag
spit dag dataset.spitin --json | spit-bash run -
```

The first command saves a plan. The second streams it to the runner and starts execution. A `.spitdag` records resolved artifact paths, jobs, commands, and dependencies. Its `version` field identifies the format. If the runner rejects the version, use matching compiler and runner versions and regenerate the plan.

[Inspecting plans](https://github.com/eclnz/spit/blob/dev/docs/guide/inspection.md) · [Command line](https://github.com/eclnz/spit/blob/dev/docs/cli.md) · [DAG format](https://github.com/eclnz/spit/blob/dev/docs/spitdag.md)

## Runs

### Run inspection

Pass `spit-bash` a recipe or a saved `.spitdag`. Use `-` to read a DAG from standard input. With a recipe, the runner makes a plan from the current dataset.

```sh
spit-bash plan examples/runnable/lines/lines.spitin --verbose
spit-bash explain 2 examples/runnable/lines/lines.spitin
```

`plan --verbose` lists each job and its reason. `current` means it can skip. `may run` means the decision depends on an upstream output. `blocked` names a prerequisite that prevents work. Use a job ID from the plan with `explain` to see the recorded and current work, checks, and file changes.

### Rerun decisions

A job can rerun after its command, inputs, or tool identity changes, or when an output is missing or changed. `explain` compares the recorded and current job fingerprints and identifies what changed.

By default, the runner compares size and modification time first, then checks content where those differ. `--compare full` always checks content. `--compare stat` uses size and modification time alone.

Changing one alpha shard can rerun its sort job. If the sorted output is identical and the merge is otherwise current, the runner can skip the merge. A change limited to artifact checks can produce a `check` job that tests existing files without repeating the command.

### Selection

```sh
spit-bash plan examples/runnable/lines/lines.spitin --entity group=beta --product merged
```

This selects beta’s merge job and the sort job it needs. `--entity` filters by dimension values, `--product` by output product, and `--stage` by stage and nested stages. Selections include upstream prerequisites. `--force` reruns matched command jobs even when current. Their prerequisites still follow the normal rerun rules.

### Scheduling

Ready jobs can run together when their resource requests fit. Set the run’s core budget with `-j` or `--cores` and its memory budget with `--memory`, as in `spit-bash run dataset.spitin -j 4 --memory 8G`. A job’s `resources` declaration sets its own request.

### Execution

Commands run from the output root. The runner creates output parent directories and prepares an empty directory for each folder output.

Input checks and `verify` run before the job’s commands. Output checks follow once all declared outputs exist. A missing output fails the job even if its command exits successfully. Check the job log for the command and its output.

A block can propagate to dependent jobs. By default, a blocked job prevents the run from starting. Use `run --partial` to let unblocked work proceed. If a run is interrupted, inspect its log and summary, when available, before replanning.

### Reuse

- **Tool identities** — `accept-tools` updates records whose only difference is the tool identity.
- **Existing outputs** — `adopt` marks jobs current when all their inputs and outputs exist. Use it for outputs you know were made by the commands in the DAG.
- **Moved outputs** — `plan --move-files --verbose` shows eligible moves as `place`. Use `run --move-files` when the outputs remain valid at their new paths.

### Maintenance

Each run gets a directory under `.spit-bash/runs/` with its plan, summary, and job logs. The summary counts outcomes. `spit-bash log dataset.spitin --product merged` lists logs for the selected work, newest first.

- **Restore** — `spit-bash restore dataset.spitin --run RUN` restores outputs replaced by that run.
- **Prune** — `spit-bash prune dataset.spitin --keep-runs N` keeps the newest runs and the logs behind current outputs. Pruned runs lose their saved replacement copies.
- **Clean** — `spit-bash clean dataset.spitin FOLDER` lists leftover files and folders. Use a folder reserved for pipeline outputs. Add `--yes` to delete the listed items.

[Runner reference](https://github.com/eclnz/spit/blob/dev/runner/README.md) · [Runner commands](https://github.com/eclnz/spit/blob/dev/runner/src/spit_bash/cli.py)

Coverage was checked against the language reference and the compiler and runner command-line entry points. A full implementation and test audit is still open.

[Language reference](https://github.com/eclnz/spit/blob/dev/docs/language-reference.md) · [Compiler CLI](https://github.com/eclnz/spit/blob/dev/src/cli/args.rs) · [Runner CLI](https://github.com/eclnz/spit/blob/dev/runner/src/spit_bash/cli.py)
