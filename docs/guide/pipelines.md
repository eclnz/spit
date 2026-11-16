# Pipelines

A pipeline is a `.spit` file. It declares the families of artifacts that exist, the operations that make new ones, and the steps that connect them. It describes the work, not the dataset: the same pipeline plans a different set of jobs for each dataset it is given. The examples come from the [weather network](walkthrough.md) and the other [showcase analyses](https://github.com/eclnz/spit-showcase).

## Syntax

A pipeline is a list of statements, and a statement's first word says what it is. The words that start statements are reserved: no product may be called `use`, `root`, `output`, `source`, `discover`, `operation`, `command`, `verify`, `resources`, `tool`, `uses`, `tools`, `anchored`, `check`, `review`, `require`, `exclude`, `path`, `ext`, `default`, `entities`, `stage` or `dimensions`. `drop`, `skip`, `sidecars` and `shell-source` start a line only to give an error that says what to write instead. Use descriptive names for products, since they appear in paths, plans and messages.

- An unquoted `#` at the start of a word begins a comment. A `#` inside a word, as in `--color=#fff`, is kept.
- A keyword alone with a `:` opens a block of its statements, each indented beneath it without repeating the keyword. The classifier analysis could write its sources like this:

```spit
source:
    dataset : Table .csv [dataset]
    model : Spec .json [model]
    split_py : Python .py
```

- `source:`, `operation:`, `command:`, `verify:`, `resources:`, `tool:`, `uses:`, `anchored:`, `check:`, `review:` and `path:` open blocks in a pipeline, and `path:`, `require:`, `exclude:` and `review:` in a recipe. [Defaults](#defaults) have a block of their own, `default:`. The entries of a block share one indentation, and the next line that is not indented beneath the keyword ends it. Each entry is read as its keyword's statement would be, with the same errors.
- A statement carries on to the next lines while its brackets are open, so a long signature or a step with many arguments can be laid out one item to a line, as the weather figure's call is.
- A command after its `:`, in a `command`, `verify` or `check` line, carries on over the lines indented beneath it, and a line break separates its words as a space does:

<!-- spit-doc: setup
operation register(moving: Image, reference: Image) -> Image
-->

```spit
command register: flirt -in {moving}
    -ref {reference}
    -out {@output}
```

Beneath any other statement, a line indented further is a statement of its own.

In the [weather family](weather.md):

- A dataset named by the pipeline: `root ../network` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- An output root named by the pipeline: `output ../output/v1` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A block of sources: `source:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of path rules: `path:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of checks: `check:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of defaults: `default:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of commands: `command:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of resources: `resources:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of tools used: `uses:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A block of operations: `operation:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A block of verifications: `verify:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A block of reviews: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)

## Steps

A step applies an operation to products and names what it makes. The terrain analysis stitches each region's tiles and then derives a hillshade, a colour relief and slopes from the result:

<!-- spit-doc: setup
source grid : Options .opt [region]
source tile : Dem .tif [region, x, y]
source ramp : Ramp .txt
operation warp(grid: Options, tiles: many Dem) -> Dem .tif
operation hillshade(dem: Dem) -> Png .png
operation colour(dem: Dem, ramp: Ramp) -> Png .png
operation slope(dem: Dem) -> Dem .tif
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
dem = warp(grid, tile @vary(x, y))
shade = hillshade(dem)
colours = colour(dem, ramp)
slopes = slope(dem)
```

The product on the left is new, and its type and dimensions come from the operation and its inputs. Each `hillshade` job depends on the `warp` job that makes the DEM it reads, so the order of the jobs follows from the products and not from the order of the lines.

- Declare an operation before its first call.
- A step may say what it makes, as `region_mean : Mean [region] = average(site_mean @vary(site))`. SPIT checks the type and dimensions against the step, and the dimensions must be in the pipeline's [dimension order](artifacts.md#dimension-order). The annotation checks the order; it does not set it.
- Products, operations and dimensions have separate names, so a dimension may share a product's name, as in `model @each(model)`. A product may share its operation's name, but `spit check` warns: name what the step makes, as in `coregistered = coreg(mc, brain)`.
- Steps can appear in any order, and a step can read a product that a later line makes.
- A step cannot read its way back to its own output. SPIT reports the loop once, naming it:

<!-- spit-doc: output
spit check cycle.spit || true
--- cycle.spit
source cleaned : Readings [region, site]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation blend(a: Readings, b: Readings) -> Readings
command blend: blend_tool {a} {b} {@output}
smoothed = blend(cleaned, adjusted)
adjusted = blend(cleaned, smoothed)
-->

```text
error: line 6, column 1: pipeline cycle: smoothed -> adjusted -> smoothed
```

Break the loop by removing one dependency, or by adding a separate input.

In the [weather family](weather.md):

- A step whose output is described: `region_mean : Mean [region] = average(site_mean @vary(site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A step written over several lines: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Matching

A job needs one artifact for each of its inputs. SPIT decides which, using dimensions:

- The input with the most dimensions **drives** the step. Each artifact of the driving input that exists is one job, and the job's output takes the driving input's dimensions.
- Every other input is matched at the driving artifact's dimension values.
- An input with no dimensions matches every job.
- A single-artifact input may use only dimensions the driving input has, unless it [broadcasts](#broadcasting) one with `@each`. SPIT checks this before it reads any data.

The weather network compares each site with its own region. `reduced` has `[region, site]` and `regional_reduced` only `[region]`:

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
operation reduce(series: Readings<Clean>) -> Readings<SeaLevel>
operation average(series: many Readings<S>) -> Readings<S>
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
reduced = reduce(cleaned)
regional_reduced = average(reduced @vary(site))
-->

```spit
operation compare(readings: Readings<SeaLevel>, baseline: Readings<SeaLevel>) -> Anomaly
anomaly = compare(reduced, regional_reduced)
```

Each `reduced[region=...,site=...]` is a job, and the job reads the one baseline whose region is the same. SPIT never picks a file silently. A job whose input has no match is an error that names the input and the values. Here a site's `location` file was misspelt `Site.json`, so no file matches the source rule:

```text
error: weather.spit: line 43, column 31: no `location` artifact for input `location` of `reduce` at [region=coast,site=dune]
```

A match can also be ambiguous. If a baseline came in revisions, every site would match several:

<!-- spit-doc: output
spit dag compare.spit compare.spitout || true
--- compare.spit
source reduced : Readings<SeaLevel> [region, site]
source baseline : Readings<SeaLevel> [region, revision]
path reduced: reduced/{region}/{site}.csv
path baseline: baseline/{region}/{revision}.csv
operation compare(readings: Readings<SeaLevel>, baseline: Readings<SeaLevel>) -> Anomaly
command compare: compare_tool {readings} {baseline} {@output}
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}.csv
anomaly = compare(reduced, baseline @same(region))
--- compare.spitout
sources:
    reduced[region=coast,site=dune]
    baseline[region=coast,revision=1]
    baseline[region=coast,revision=2]
-->

```text
error: line 9, column 28: more than one `baseline` artifact matches input `baseline` of `compare` at [region=coast,site=dune]; select one with `@where(...)`
```

The first step in finding a missing or ambiguous match is to look at the candidates: [Plans](plans.md#missing-work) shows how. Then add a selector that leaves one:

- `baseline @where(revision=2)` keeps the artifacts with that value and takes `revision` out of matching.
- `baseline @same(region)` matches on `region` alone, as in the example above. If two baselines still match, the job is ambiguous.

The weather figure uses `@where` to pick the one site and day that panel **b** draws: `reading @where(region=coast, site=dune, day=2025-06-26)`. A selector can sit beside a collection, as in `frame @where(acq=fast) @vary(run)`, which filters first and then gathers. Every `@` clause has its keyword directly after the `@`: `@ vary(day)` is an error that says to write `@vary(...)`.

In the [weather family](weather.md):

- An input that pins values: `against_dune = compare(site_mean, site_mean @where(region=coast, site=dune))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- An input matched on one dimension: `against_region = compare(site_mean, region_mean @same(region))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### First and last artifacts

Use `@first(session)` on a single-artifact input to take the earliest available session for each subject. Use `@last(session)` for the latest.

```spit
source scan : Image .nii.gz [subject, session]
path scan: scans/{subject}/{session}.nii.gz
operation copy(image: Image) -> Image .nii.gz
command copy: cp {image} {@output}
default path: out/{@product}/{subject}
baseline = copy(scan @first(session))
followup = copy(scan @last(session))
```

Both outputs have only `[subject]`. SPIT orders session values [as it orders collections](#collections), so a subject with sessions `2` and `10` gets `2` as its baseline. Each job reads only the scan it selects. Adding a later scan leaves that subject's baseline job unchanged.

You can filter another dimension first. For an input with `[subject, session, acquisition]`, `scan @where(acquisition=fast) @first(session)` takes the earliest fast scan of each subject.

## Collections

A `many` port takes a group of artifacts, and the step says which dimensions to gather. The weather network gathers each site's days into one series, and then reuses one `average` operation for three groupings:

<!-- spit-doc: setup
source reading : Readings<Raw> [region, site, day]
source calibration : Calibration [region, site]
operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
cleaned = clean(reading @vary(day), calibration)

operation average(series: many Readings<S>) -> Readings<S>
regional = average(cleaned @vary(site))
overall = average(cleaned @vary(region, site))
```

`@vary(day)` gathers every `day` within each `region` and `site`, so `cleaned` has one artifact for each site and no `day` dimension. `@vary(site)` gathers each region's sites, so there is one `regional` for each region. `@vary(region, site)` gathers every site into one `overall`. A bound on the port, such as `@min(2)`, is checked for each group:

<!-- spit-doc: output
spit dag clean.spit clean.spitout || true
--- clean.spit
source reading : Readings<Raw> [region, site, day]
path reading: raw/{region}/{site}/{day}.csv
source calibration : Calibration [region, site]
path calibration: raw/{region}/{site}/calibration.json
operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>
command clean: clean_tool {days} {calibration} {@output}
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}.csv
cleaned = clean(reading @vary(day), calibration)
--- clean.spitout
sources:
    reading[region=coast,site=dune,day=2025-01-01]
    calibration[region=coast,site=dune]
-->

```text
error: line 9, column 17: input `days` of `clean` needs at least 2 artifacts at [region=coast,site=dune], found 1
```

Several rules decide how a collection behaves:

- **Order.** A collection follows the pipeline's [dimension order](artifacts.md#dimension-order), whatever order `@vary` lists its dimensions in. Within a value, runs of digits compare as numbers and other characters one by one, so take `2` comes before take `10`, ISO dates such as `2025-06-26` sort by date, and names sort by character. Values equal as numbers but written differently, such as `1` and `01`, are then ordered by their text. The command's arguments are in the same order.
- **Whole arguments.** `{days}` expands to one argument for each artifact, so it must be a whole argument of its own. To put the paths in one argument, use [`@join` or `@list`](operations.md#collections-as-one-argument).
- **Several dimensions.** `@vary(region, site)` gathers over both in one clause. A call may have only one `@vary`, and `@min` counts the whole collection.
- **Bounds.** `@min(2)` goes on the port it counts, as `days: many Readings @min(2)`, or `days: many @min(2)` untyped. After the outputs, it is an error that shows the line rewritten.
- **Other inputs.** A `many` port can sit beside ordinary ports, which are matched once for each group, as `calibration` is above.
- **One port, several lists.** `{anomalies.region}` and `{anomalies.site}` give the matching region and site of each artifact, in the same order as `{anomalies}`, so a script can pair them. The weather network's ranking uses this.
- **Parallel collections.** An operation that takes two `many` ports must be given the same `@vary` on both. The two collections must have the same dimensions and the same identities, and their placeholders list artifacts in the same order, so the lists pair up by position. An artifact in one and not the other is an error that names it.
- **Two groupings of one input.** The classifier analysis ranks its scores twice, with `scores @vary(model)` to rank models on each dataset and `scores @vary(dataset)` to rank datasets for each model.

In the [weather family](weather.md):

- A step that gathers a dimension: `climate = site_mean(day_means @vary(day))` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A step that gathers several dimensions: `ranking = league(site_mean @vary(region, site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

## Broadcasting

`@each` is the reverse of `@vary`: it runs a step once for every value of a dimension that the driving input lacks, so the output gains that dimension. The weather network checks every site against every alert:

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
source threshold : Threshold .json [alert]
operation tally(series: Readings<Clean>, threshold: Threshold) -> Count
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
dimensions [region, site, alert]
alert_days = tally(cleaned, threshold @each(alert))
```

With 16 sites and two alerts, this plans 32 jobs, one for each `alert_days[region=...,site=...,alert=...]`. The values come from the artifacts that exist, so a third alert file would add 16 jobs and change nothing in the pipeline. The classifier analysis crosses every model with every dataset the same way, `fit(fit_py, model @each(model), train)`, so adding a model is adding a file.

- Only one input may broadcast a given dimension, and the driving input must not already have it.
- Broadcasting never invents a pair that the driving input lacks. If the readings of one site are missing a day, no job is made for it.
- No source holds both `site` and `alert`, so the `dimensions` line sets their order.
- A later `@vary(alert)` collects the results again.

In the [weather family](weather.md):

- A step that broadcasts a dimension: `shift @each(scenario)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Composite operations

An operation can be carried out by steps instead of a command. Its header ends in `:` and its body is indented beneath it. The body reads the inputs by their port names, and assigns each named output. The weather network's figure is one: six steps draw a panel each, and a seventh lays them out.

<!-- spit-doc: setup
operation map(terrain: Terrain, sites: many Site) -> Panel .png
operation cleaning(raw: Readings<Raw>, calibration: Calibration, clean: Readings<Clean>) -> Panel .png
operation means(regions: Table<Regions>, days: Table<Days>) -> Panel .png
operation anomalies(anomalies: many Anomaly) -> Panel .png
operation alert_days(alerts: Table<Alerts>) -> Panel .png
operation drift(league: Table<League>) -> Panel .png
operation compose(a: Panel, b: Panel, c: Panel, d: Panel, e: Panel, f: Panel) -> Figure .png
-->

```spit
operation figure(
    terrain: Terrain,
    sites: many Site,
    raw: Readings<Raw>,
    calibration: Calibration,
    clean: Readings<Clean>,
    regions: Table<Regions>,
    days: Table<Days>,
    alerts: Table<Alerts>,
    anomalies: many Anomaly,
    league: Table<League>
) -> (figure: Figure):
    a = map(terrain, sites)
    b = cleaning(raw, calibration, clean)
    c = means(regions, days)
    d = anomalies(anomalies)
    e = alert_days(alerts)
    f = drift(league)
    figure = compose(a, b, c, d, e, f)
```

A call to `figure` behaves like any other call, and expands into its body's steps over the caller's products. A body may call another composite operation, and a selector the caller gives an input holds wherever the body reads it.

- **Outputs.** The header names each output, as `-> (figure: Figure)`, since the body assigns them by name, and the body makes each one once. An output takes its extension, folder and path from the step that writes it, so the header gives only its name and type. A composite operation has no `command` or `verify` line of its own; the operations its steps call have theirs.
- **The body's own products.** A product the body makes for itself, such as `a`, is filed under the call's first output: `figure::a`, written `figure.a` in a path. A second call files its own apart. Only the outputs belong to the caller, so a step that reads `figure::a` is an error that says to make it an output.
- **Types.** A call is held to the operation's signature like any other call: a type variable is one type throughout the call, and an output written with a type is checked against the step that makes it.
- **Checks.** A check on one of the operation's ports runs on every step that reads that input or makes that output, beside the steps' own checks.
- **Stages.** A call inside a stage puts every step it makes in that stage.

`spit check` lists the expanded steps before any data is read, and `spit dag --counts` shows the jobs each step makes:

<!-- spit-doc: plain -->

```text
figure = fig::figure(…)  line 78  [plot]
  fig::figure  figure.spit  blob ae1de64
  line 37  figure::a = fig::map(terrain, location)
  line 38  figure::b = fig::cleaning(reading, calibration, cleaned)
  line 39  figure::c = fig::means(region_table, day_table)
  line 40  figure::d = fig::anomalies(anomaly)
  line 41  figure::e = fig::alert_days(alert_table)
  line 42  figure::f = fig::drift(league)
  line 43  figure = fig::compose(figure::a, figure::b, figure::c, figure::d, figure::e, figure::f)
```

An error inside a body is reported at the call. It points at the argument the caller gave when the failing input reads one of the operation's inputs, at the product the caller names when it is an output, and at the whole call otherwise. A `-->` line beneath it gives each call it is nested in, and the body's step:

<!-- spit-doc: output
spit check p1.spit || true
--- libs/lib.spit
operation clean(x: Lines, t: Table) -> Lines
command clean: clean {x} {t} {@output}
operation merge(xs: many Lines) -> Lines
command merge: merge {xs} {@output}
operation count(x: Lines) -> Count
command count: wc {x} {@output}

operation tidy(x: Lines, t: Table) -> (y: Lines):
    # one clean step
    y = clean(x, t)

operation summarise(reads: Lines, table: Table) -> (merged: Lines, total: Count):
    cleaned = tidy(reads, table)
    merged = merge(cleaned @vary(lane))
    total = count(merged)
--- p1.spit
use libs/lib.spit as L
source raw : Lines [lane]
source cal : Table
m, t = L::summarise(cal, raw)
-->

```text
error: line 4, column 21: in `m, t = L::summarise(...)`: type mismatch at `L::clean.x`: product `cal` is Table, expected Lines
  --> libs/lib.spit: line 13, column 15: the call of `L::tidy` in the body of `L::summarise`
  --> libs/lib.spit: line 10, column 9: the step in the body of `L::tidy`
```

`spit dag --commands` starts each job a call made with a `from:` line naming the calls it is nested in, outermost first, then the body's step.

In the [weather family](weather.md):

- An operation carried out by steps: `operation what_if(script: Script, mean: Mean, shift: Shift) -> (result: Mean):` in [stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/stats.spit)
- A call to an imported composite operation: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Stages

A stage groups the steps of one phase and gives their products a shared path. The MRI quality-control pipeline has three:

<!-- spit-doc: setup
source t1w : T1w .nii.gz [sub]
check nonempty: test -s {@path}
check usable(min_snr): test -s {@path} {min_snr}
-->

```spit
default entities: {@key}-{@value} separated "_"
default path: out/{@stage}/{@product}[/{@entities}]

stage inspect:
    operation mosaic(t1: T1w) -> Png .png
    command mosaic: python3 {@here}/scripts/mosaic.py {t1} {@output}
    mosaics = mosaic(t1w)

stage measure:
    operation summarise(t1: T1w) -> Json .json
    command summarise: python3 {@here}/scripts/summarise.py {t1} {@output}
    summaries = summarise(t1w)

stage qc:
    operation approve(summary: Json) -> Json .json @check(usable(35))
    command approve: cp {summary} {@output}
    approved = approve(summaries)
```

- A stage is indented beneath `stage name:`, and stages nest. A product's `{@stage}` is its stage path, such as `preprocess/combine`, so `approved` is `out/qc/approved/sub-01.json`.
- A stage can set defaults, such as `default path:` or `default check:`, for its own products. A product-specific rule still takes precedence.
- Stages do not order the jobs. Product dependencies do, so `qc` reads `summaries` by name, and two stages may even read from each other.
- A second `stage qc:` header at the same level continues the first block: its steps are in the same stage.
- A product made outside every stage has no `{@stage}`, so a default that covers it writes the stage as an [optional part](artifacts.md#optional-parts), `[{@stage}/]`.
- Operation names are global, including those declared inside a stage. Declare an operation in the stage that holds its calls, or at the top level, or SPIT warns.
- Sources and `use` imports belong at the top level.

`spit dag` counts the jobs in each outermost stage, and `spit-bash` can run one stage on its own with `--stage`. `spit dag --tree` shows how the stages connect.

In the [weather family](weather.md):

- A stage: `stage clean:` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A stage inside a stage: `stage site:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Defaults

A `default` line sets something for every product of the file, or of the stage it is written in, that has no setting of its own. The weather network's v3 sets five in one block:

<!-- spit-doc: setup
check nonempty: test -s {@path}
tool awk
-->

```spit
default:
    entities: {@key}={@value} separated "_"
    entities site: place
    path: {@stage}/{@product}[/{@entities}]
    ext: .txt
    check: nonempty
    uses: awk
```

| Default | Sets | Explained in |
| --- | --- | --- |
| `default path:` | The path rule of each product with none of its own | [Paths](artifacts.md#paths) |
| `default ext:` | The extension that completes a default path rule | [Extensions](artifacts.md#extensions) |
| `default entities:` | How `{@entities}` is spelled | [Entity spelling](artifacts.md#entity-spelling) |
| `default check:` | The checks every output runs | [Default checks](operations.md#default-checks) |
| `default review:` | The reviews that judge every output | [Human review](datasets.md#human-review) |
| `default uses:` | The tools of every operation declared there | [Tools](operations.md#tools) |

- A default outside every stage covers the whole file, stages included. One inside a stage covers that stage and the stages nested in it.
- For a path, an extension or a spelling, the nearest default wins. Check, review and tool lists add up: the file's first, then each stage's from the outermost in. `!name` drops one that a wider list added.
- A file or a stage sets each default once, on its own line or in the block. Inside `default:`, each line is one of those six without the word `default`.
- `path:`, `ext:`, `check:` or `entities:` written alone, outside a `default:` block, is an error that says how to write it.
- A recipe takes `default path:`, for its sources, and `default entities:`, for the rules it gives. The other defaults belong in the pipeline.

## Imports

`use` brings definitions in from another `.spit` file, so a library can hold operations that many pipelines share. The weather network keeps its figure in `figure.spit`:

<!-- spit-doc: fragment -->

```spit
use figure from figure.spit as fig
figure = fig::figure(terrain, location @vary(region, site), reading, calibration, cleaned,
    region_table, day_table, alert_table, anomaly @vary(region, site), league)
```

The path is relative to the importing file. `as fig` gives every imported name the prefix `fig::`, which also resolves a clash between two libraries. Without `from`, everything the file defines is imported, and without `as`, the names come in unqualified. The text-processing example shows the other form, importing a source and an operation by name:

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
--- text.spit = examples/imports/text.spit
-->

```spit
use shard, sort_lines from text.spit as text
sorted = text::sort_lines(text::shard)
```

- An imported operation carries its command or body, its resources, its tools, its checks and its reviews, along with the operations its body calls. An imported source carries its path rule, its checks, its reviews and its sidecar companions. A companion cannot be imported alone: import its main source.
- With `as`, every imported name takes the prefix, checks and reviews included, as in `@check(img::ndim(3))`.
- The other file's top-level steps stay in that file, so a library adds no jobs to the pipeline that uses it.
- An unknown name, an import cycle and a name collision are each an error. Use distinct aliases for a collision, and remove the cycle.
- A recipe names an imported source with its prefix, as `text::shard`.

An error in a library's own text is reported in the library, at its own line, and the `use` line that reads it follows:

<!-- spit-doc: output
spit check pipeline.spit || true
--- libs/text.spit
operation sortd(input: Lines) -> Lines
command sortd: sort {input} {@output}
operation wrap(x: Lines) -> (summary: Lines):
    summary = sortd(nope)
--- pipeline.spit
use libs/text.spit as text
source raw : Lines [id]
-->

```text
error: libs/text.spit: line 4, column 15: the body of `wrap` reads `nope`, which is neither one of its inputs nor made by an earlier step of it
  --> pipeline.spit: line 1, column 1: imported here
```

A message names a library by its path from the pipeline's folder, wherever the checkout is.

Next: [Datasets](datasets.md).

In the [weather family](weather.md):

- Every name in a file, imported: `use ../shared/stats.spit` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A file's names under an alias: `use ../shared/stats.spit as stats` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- Chosen names from a file: `use nonempty, average, what_if from ../shared/stats.spit as stats` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- An imported check: `check: stats::nonempty` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
