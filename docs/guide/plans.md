# Plans

SPIT turns a pipeline and a dataset into a **plan**: the list of jobs, with every path and command filled in. You can look at the plan before anything runs, and when a job you expect is missing, SPIT can say why. This page covers the commands that show a plan, and a method for finding missing work.

A useful order is to check the pipeline, check what SPIT found in the dataset, look at the jobs, and only then save the plan:

<!-- spit-doc: fragment -->

```sh
spit check pipeline.spit --path-rules
spit inputs dataset.spitin --unmatched
spit dag dataset.spitin --counts --commands
spit dag dataset.spitin -o plan.spitdag
```

## Pipeline checking

`spit check` reads a pipeline, a recipe, an inventory or a tools file and reports errors at their source locations. It reads no data, so it is quick enough to run on every save.

<!-- spit-doc: output
spit check examples/runnable/lines/lines.spit --path-rules
-->

```text
Product path coverage:
  shard (source): explicit input/{group}/{part}.txt
  sorted (output): default output/{@product}/group={group}_part={part}.txt
  merged (output): default output/{@product}/group={group}.txt
Pipeline valid.
```

- It reports syntax, type, dimension, command and path errors.
- `--path-rules` lists the effective path rule for each product and where the rule came from.
- `--calls` lists the expanded steps of every [composite operation](pipelines.md#composite-operations).
- On a recipe, it checks the recipe's rules against its pipeline.

`spit check --json --hovers` returns structured diagnostics, path hints and hover text, which is how the [VS Code extension](https://github.com/eclnz/spit-vscode) shows inferred types, producers and paths on hover. Add `--stdin` to check unsaved text while keeping the file's path, so imports still resolve.

## Pipeline topology

`spit dag` on a pipeline alone, with `--tree`, shows how products connect before any dataset is chosen:

<!-- spit-doc: output
spit dag examples/runnable/lines/lines.spit --tree
-->

```text
Pipeline
     [shard]
        │
        ▼
  (sort_lines)
        │
        ▼
    [sorted]
        │
        ▼
     (merge)
        │
        ▼
    [merged]
```

Square brackets are products and parentheses are operations. Follow `sorted` from the sort step to the merge step. A pipeline with stages first shows an overview of the products that connect stages, and then a diagram for each stage. A join shows one arrow for each input, and a composite operation is one node, followed by a diagram of its body. Use `--ascii` for a terminal that cannot draw boxes.

## Job resolution

Give `spit dag` a recipe, and it settles the dataset in memory and plans the jobs:

<!-- spit-doc: output
spit dag examples/runnable/lines/lines.spitin --counts
-->

```text
jobs  step
   3  sorted = sort_lines
   2  merged = merge
   5  total
```

`--counts` lists the jobs for each step, including steps with zero jobs, and a zero is often the first sign that a step's driving source has no files. Three views show more:

- `--commands` shows each job's command with its paths filled in, with `verify` and `check` lines and a `needs:` line when the job declares resources.
- `--jobs` lists each job's inputs and outputs.
- `--paths` lists the artifact paths in the plan.

`--counts` can precede either job view, and `--paths` and `--commands` can print together. With `-o`, only `--counts` and `--commands` also print, and `--paths` conflicts with `-o`.

## Missing work

When a job you expect is missing, follow the chain from the files to the plan. Each step narrows the cause.

**1. Check discovery.** `spit inputs` reports how many source artifacts it found. A rule may match no files, or capture an unexpected value, such as a date in a name where a number was expected. `--unmatched` lists the files that no source rule reads:

<!-- spit-doc: fragment -->

```sh
spit inputs dataset.spitin --unmatched
```

**2. Trace the target.** `spit artifacts` reports every artifact the plan can and cannot make, with the cause of each gap, and the sources that no job uses. A dataset where one folder is spelled `Beta` in the shards and `beta` in the references shows the usual case:

<!-- spit-doc: output
spit artifacts dataset.spitin
--- dataset.spitin
pipeline pipeline.spit
root .
--- pipeline.spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt
source ref : Lines [group]
path ref: ref/{group}.txt
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)
operation compare(items: many Lines, ref: Lines) -> Lines
command compare: cmp {ref} {items} {@output}
report = compare(sorted @vary(part), ref)
--- input/alpha/01.txt
--- input/alpha/02.txt
--- input/Beta/01.txt
--- input/gamma/01.txt
--- ref/alpha.txt
--- ref/beta.txt
--- ref/gamma.txt
-->

```text
Incomplete artifacts: 1
  report[group=Beta] : Lines  (compare)
    - no `ref` artifact for input `ref` of `compare` at [group=Beta]
    ref[group=beta] exists; its `group` differs only in letter case

Unused sources: 1
  ref[group=beta] : Lines
```

Values match exactly as written, so `Beta` and `beta` are different groups. The unused source is the near miss, and SPIT points at it. `spit artifacts --by-target` groups the incomplete artifacts under the final products they prevent, which is shorter when one missing input holds up many.

**3. Read the plan.** `spit dag --counts` shows the jobs per step, `--commands` the commands and `--paths` the paths. A path that is not where you expect points to a path rule, and `spit check --path-rules` shows which rule applies.

Plain `spit dag` stops at the first job it cannot complete, and names `spit artifacts` and `--partial` as the next step.

## Partial planning

When a plan has gaps that you accept for now, `--partial` keeps the jobs whose inputs are complete and leaves out the rest:

<!-- spit-doc: fragment -->

```sh
spit dag dataset.spitin --partial -o plan.spitdag
```

The plan lists each omitted output and the reason in its `left_out` array, so a runner can report what is missing. A collection keeps its complete members and then applies `@min(n)` to what remains: a group with one complete member still fails `@min(2)`.

`--partial` is a choice for one planning run, not a rule about the data. To drop a group for a stated reason, use [`exclude`](datasets.md#exclusions). To make missing data an error, use [`require`](datasets.md#coverage-requirements).

## DAGs

The plan is saved as a `.spitdag` file, and a runner needs nothing else:

<!-- spit-doc: fragment -->

```sh
spit dag dataset.spitin -o plan.spitdag
spit dag dataset.spitin --json | spit-bash run -
```

The first command saves a plan. The second streams it into the runner, which starts executing at once. A `.spitdag` holds the resolved artifact paths, the jobs with their commands and checks, and the dependencies between them. Its `version` field identifies the format, and a runner reads exactly the version it was written for. If `spit-bash` rejects a plan's version, use a `spit` and a `spit-bash` from the same release and make the plan again.

Make the plan again whenever the pipeline or the dataset changes. `spit-bash` can also take a recipe directly and make the plan itself, so the saved file is only needed to keep one plan, review it, or hand it to another runner. The [DAG format](../spitdag.md) defines every field.

Next: [Runs](runs.md).
