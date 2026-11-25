# Plans

SPIT turns a pipeline and a dataset into a **plan**: the list of jobs, with every path and command filled in. You can look at the plan before anything runs, and when a job you expect is missing, SPIT can say why. This page covers the commands that show a plan, and a method for finding missing work. The examples come from the [weather network](walkthrough.md).

A useful order is to check the pipeline, check what SPIT found in the dataset, look at the jobs, and only then save the plan:

<!-- spit-doc: fragment -->

```sh
spit check weather.spit --path-rules
spit inputs weather.spitin --unmatched
spit dag weather.spitin --counts --commands
spit dag weather.spitin -o weather.spitdag
```

## Pipeline checking

`spit check` reads a pipeline, a recipe, an inventory or a tools file and reports errors at their source locations. It reads no data, so it is quick enough to run on every save.

- It reports syntax, type, dimension, command and path errors.
- `--path-rules` lists the effective path rule for each product and where the rule came from.
- `--calls` lists the expanded steps of every [composite operation](pipelines.md#composite-operations).
- On a recipe, it checks the recipe's rules against its pipeline.

`--path-rules` on the weather pipeline shows what every product's file name will be before there is a dataset:

<!-- spit-doc: plain -->

```text
Product path coverage:
  location (source): default {region}/{site}/location.json, `.json` from source `location`
  calibration (source): default {region}/{site}/calibration.json, `.json` from source `calibration`
  cleaned (output): default {region}/{site}/cleaned.csv, `.csv` from `default ext:`
  regional (output): default {region}/regional.csv, `.csv` from `default ext:`
  league (output): default league.csv, `.csv` from `default ext:`
  alert_days (output): default {region}/{site}/alert_days_{alert}.csv, `.csv` from `default ext:`
  figure::b (output): default figure.b.png, `.png` from operation `fig::cleaning`
  figure (output): default figure.png, `.png` from operation `fig::compose`

Pipeline valid.
```

The lines are an excerpt. The recipe's source paths replace the first two when you check the recipe instead.

`spit check --json --hovers` returns structured diagnostics, path hints and hover text, which is how the [editor](editor.md) shows inferred types, producers and paths on hover. Add `--stdin` to check unsaved text while keeping the file's path, so imports still resolve.

## Pipeline topology

`spit dag` on a pipeline alone, with `--tree`, shows how products connect before any dataset is chosen:

<!-- spit-doc: plain -->

```text
Pipeline overview
Stages: clean, compare, alerts, plot
(clean) ──[cleaned]──> (compare)
(clean) ──[cleaned]──> (alerts)
(clean) ──[cleaned]──> (plot)
(compare) ──[regional, overall, anomaly, league]──> (plot)
(alerts) ──[alert_days]──> (plot)

Stage: clean
  [reading]   [calibration]
      │             │
      ▼             ▼
┌───────────────────────────┐
│           clean           │
└─────────────┬─────────────┘
              │
              ▼
          [cleaned]
```

The overview names the products that connect the stages. A diagram for each stage follows: square brackets are products, boxes are operations, and a join shows one arrow for each input. A composite operation is one node, followed by a diagram of its body.

## Job resolution

Give `spit dag` a recipe, and it settles the dataset in memory and plans the jobs. The cohort example is small enough to read in full:

<!-- spit-doc: output
spit dag examples/patterns/cohort/cohort.spitin --counts
-->

```text
jobs  step                 stage
   4  brain = bet          anat
   7  mc = motioncorr      func
   7  coreg = register     func
   4  avg = sessionavg     func
   2  long = longitudinal
  24  total
```

The four sessions give four `bet` jobs and four session averages. Subjects 01 and 02 have eight BOLD runs, and the recipe excludes the one with a motion spike, so seven runs give seven motion-correction and registration jobs. Subject 03 has one session and is excluded as a whole, so the two longitudinal jobs are subjects 01 and 02. `--counts` lists the jobs for each step, including steps with zero jobs, and a zero is often the first sign that a step's driving source has no files. Three views show more:

- `--commands` shows each job's command with its paths filled in, with `verify` and `check` lines and a `needs:` line when the job declares resources.
- `--jobs` lists each job's inputs and outputs.
- `--paths` lists the artifact paths in the plan.

`--counts` can precede either job view, and `--paths` and `--commands` can print together. With `-o`, only `--counts` and `--commands` also print, and `--paths` conflicts with `-o`. For the weather network, a `tally` job looks like this:

<!-- spit-doc: plain -->

```text
Job 65  tally  [alerts]
  check:  python3 -m json.tool ../raw/alerts/frost.json
  run:    python3 /path/to/weather/steps/tally.py --alert frost --threshold ../raw/alerts/frost.json -o coast/thrift/alert_days_frost.csv coast/thrift/cleaned.csv
  check:  test -s coast/thrift/alert_days_frost.csv
```

The input check runs before the command, and the output check after it. The paths are relative to `data/results`, where the commands run.

## Missing work

When a job you expect is missing, follow the chain from the files to the plan. Each step narrows the cause. The example is a weather network where one site's location file is named `Site.json` instead of `site.json`.

**1. Check discovery.** `spit inputs` reports how many source artifacts it found, and a rule that matches too little shows as a count lower than the files on disk. `--unmatched` lists the files that no source rule reads:

<!-- spit-doc: plain -->

```text
$ spit inputs weather.spitin --unmatched
coast/dune/Site.json
```

Here the recipe's `require` rules already stop the plan at the same place, naming the site: `source coverage for `location` at [region=coast,site=dune]: expected exactly 1 artifact(s), found 0`. A pipeline without them fails later, at the step that reads the file, and says so:

<!-- spit-doc: plain -->

```text
error: weather.spit: line 43, column 31: no `location` artifact for input `location` of `reduce` at [region=coast,site=dune]
  10 more artifacts cannot be produced; run `spit artifacts` to list them, or `spit dag --partial` to plan the rest
```

**2. Trace the target.** One missing file may hold up a lot. `spit artifacts --by-target` groups every artifact that cannot be made under the final product it prevents, and gives the first cause:

<!-- spit-doc: plain -->

```text
Final targets that cannot be made: 1 (incomplete artifacts: 11)
  figure : Figure  (fig::compose, stage plot, in `figure = fig::figure(...)` on line 78)
    figure::d : Panel  (fig::anomalies, stage plot, in `figure = fig::figure(...)` on line 78)
      anomaly[region=coast,site=alder] : Anomaly  (compare, stage compare)
        regional_reduced[region=coast] : Readings<SeaLevel>  (average, stage compare)
          reduced[region=coast,site=dune] : Readings<SeaLevel>  (reduce, stage compare)
            - no `location` artifact for input `location` of `reduce` at [region=coast,site=dune]
```

The figure is the final product. Following the indented lines down, panel **d** needs the anomaly of a site, which needs its region's average, which needs the reduced series for `dune`, which lacks its location. Plain `spit artifacts` lists the same artifacts as a flat report, with the complete ones first and the unused sources last.

Values match exactly as written, so a near miss, such as `Beta` where `beta` is needed, shows up as an unused source that SPIT points at: ``ref[group=beta] exists; its `group` differs only in letter case``.

**3. Read the plan.** `spit dag --counts` shows the jobs per step, `--commands` the commands and `--paths` the paths. A path that is not where you expect points to a path rule, and `spit check --path-rules` shows which rule applies.

Plain `spit dag` stops at the first job it cannot complete, and names `spit artifacts` and `--partial` as the next steps.

## Partial planning

When a plan has gaps that you accept for now, `--partial` keeps the jobs whose inputs are complete and leaves out the rest:

<!-- spit-doc: fragment -->

```sh
spit dag weather.spitin --partial -o weather.spitdag
```

```text
note: planned 96 jobs; left out 2 artifacts that cannot be produced (see left_out)
```

The plan lists each omitted output and the reason in its `left_out` array, so a runner can report what is missing:

<!-- spit-doc: plain -->

```text
reduced[region=coast,site=dune]: no `location` artifact for input `location` of `reduce` at [region=coast,site=dune]
anomaly[region=coast,site=dune]: input `readings` needs reduced[region=coast,site=dune], which cannot be produced
```

A job whose collection has a member that cannot be made is left out too, as `anomaly` is above, so an aggregate never stands for fewer members than it names. To aggregate the rest, leave the member out in the recipe with a reason, and give the reduced results an output folder of their own, so they are never mistaken for the full ones:

<!-- spit-doc: recipe
--- weather.spit
source location : Site .json [region, site]
-->

```spit
pipeline weather.spit
root data/raw
output data/without-dune

inputs:
    location: {region}/{site}/site.json

exclude [region=coast,site=dune]    # its location file was lost
```

`--partial` is a choice for one planning run, not a rule about the data. To drop a group for a stated reason, use [`exclude`](datasets.md#exclusions). To make missing data an error, use [`require`](datasets.md#coverage-requirements).

## DAGs

The plan is saved as a `.spitdag` file, and a runner needs nothing else:

<!-- spit-doc: fragment -->

```sh
spit dag weather.spitin -o weather.spitdag
spit dag weather.spitin --json | spit-bash run -
```

The first command saves a plan. The second streams it into the runner, which starts executing at once. A `.spitdag` holds the resolved artifact paths, the jobs with their commands and checks, and the dependencies between them. A job from a composite operation records where it came from, in an `origin` that names the call and the line of the body's step. Its `version` field identifies the format, and a runner reads exactly the version it was written for. If `spit-bash` rejects a plan's version, use a `spit` and a `spit-bash` from the same release and make the plan again.

Make the plan again when the pipeline or the dataset changes. Human verdicts can change between invocations of the same saved DAG. `spit dag` and `spit artifacts` show where gates attach, and the runner reports what the current verdicts admit. `spit-bash` can also take a recipe directly and make the plan itself, so the saved file is only needed to keep one plan, review it, or hand it to another runner. The [DAG format](../spitdag.md) defines every field.

Next: [Runs](runs.md).
