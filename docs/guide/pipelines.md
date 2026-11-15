# Pipelines

A pipeline is a `.spit` file. It declares the families of artifacts that exist, the operations that make new ones, and the steps that connect them. It describes the work, not the dataset: the same pipeline plans a different set of jobs for each dataset it is given. The examples come from the [weather network](walkthrough.md) and the other [showcase analyses](https://github.com/eclnz/spit-showcase).

## Syntax

A pipeline is a list of statements, and a statement's first word says what it is. The words that start statements are reserved, so a product cannot be called `source`, `stage`, `path`, `default` and so on. Use descriptive names for products, since they appear in paths, plans and messages.

- An unquoted `#` at the start of a word begins a comment. A `#` inside a word, as in `--color=#fff`, is kept.
- A keyword alone with a `:` opens a block of its statements, each indented beneath it without repeating the keyword. The classifier analysis could write its sources like this:

```spit
source:
    dataset : Table .csv [dataset]
    model : Spec .json [model]
    split_py : Python .py
```

- A statement carries on to the next lines while its brackets are open, so a long signature or a step with many arguments can be laid out one item to a line, as the weather figure's call is.

In the [weather family](weather.md):

- A dataset named by the pipeline: `root network` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- An output root named by the pipeline: `output output/v1` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A block of sources: `source:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of path rules: `path:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of checks: `check:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of defaults: `default:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of commands: `command:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of resources: `resources:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of tools used: `uses:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A block of operations: `operation:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey.spit)
- A block of verifications: `verify:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey.spit)
- A block of reviews: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey.spit)

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

- A step whose output is described: `region_mean : Mean [region] = average(site_mean @vary(site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A step written over several lines: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

## Matching

A job needs one artifact for each of its inputs. SPIT decides which, using dimensions:

- The input with the most dimensions **drives** the step. Each artifact of the driving input that exists is one job, and the job's output takes the driving input's dimensions.
- Every other input is matched at the driving artifact's dimension values.
- An input with no dimensions matches every job.

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

The weather figure uses `@where` to pick the one site and day that panel **b** draws: `reading @where(region=coast, site=dune, day=2025-06-26)`. A selector can sit beside a collection, as in `frame @where(acq=fast) @vary(run)`, which filters first and then gathers.

In the [weather family](weather.md):

- An input that pins values: `against_dune = compare(site_mean, site_mean @where(region=coast, site=dune))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- An input matched on one dimension: `against_region = compare(site_mean, region_mean @same(region))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)

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

- **Order.** A collection follows the pipeline's [dimension order](artifacts.md#dimension-order), and runs of digits compare as numbers, so take `2` comes before take `10`. The command's arguments are in the same order.
- **Several dimensions.** `@vary(region, site)` gathers over both in one clause. A call may have only one `@vary`.
- **Other inputs.** A `many` port can sit beside ordinary ports, which are matched once for each group, as `calibration` is above.
- **One port, several lists.** `{anomalies.region}` and `{anomalies.site}` give the matching region and site of each artifact, in the same order as `{anomalies}`, so a script can pair them. The weather network's ranking uses this.
- **Parallel collections.** An operation that takes two `many` ports must be given the same `@vary` on both. The two collections must have the same dimensions and the same identities, and their placeholders list artifacts in the same order, so the lists pair up by position. An artifact in one and not the other is an error that names it.
- **Two groupings of one input.** The classifier analysis ranks its scores twice, with `scores @vary(model)` to rank models on each dataset and `scores @vary(dataset)` to rank datasets for each model.

In the [weather family](weather.md):

- A step that gathers a dimension: `climate = site_mean(day_means @vary(day))` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A step that gathers several dimensions: `ranking = league(site_mean @vary(region, site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)

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

- A step that broadcasts a dimension: `shift @each(scenario)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

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

A call to `figure` behaves like any other call, and expands into its body's steps over the caller's products. A product the body makes for itself, such as `a`, stays private to the call: the caller can read only the outputs. A body may call another composite operation, and a selector the caller gives an input holds wherever the body reads it.

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

An error inside a body is reported at the call, with the body's step beneath it. See [operations carried out by steps](../language-reference.md#operations-carried-out-by-steps).

In the [weather family](weather.md):

- An operation carried out by steps: `operation what_if(mean: Mean, shift: Shift) -> (result: Mean):` in [lib/stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/lib/stats.spit)
- A call to an imported composite operation: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

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
- Operation names are global, including those declared inside a stage. Declare an operation in the stage that holds its calls, or at the top level, or SPIT warns.
- Sources and `use` imports belong at the top level.

`spit dag` counts the jobs in each outermost stage, and `spit-bash` can run one stage on its own with `--stage`. `spit dag --tree` shows how the stages connect.

In the [weather family](weather.md):

- A stage: `stage clean:` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A stage inside a stage: `stage site:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

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

- An imported operation carries its command or body, its resources and its checks, along with the operations its body calls. An imported source carries its path rule, and its sidecar companions.
- The other file's top-level steps stay in that file, so a library adds no jobs to the pipeline that uses it.
- An unknown name, an import cycle and a name collision are each an error. Use distinct aliases for a collision, and remove the cycle.
- A recipe names an imported source with its prefix, as `text::shard`.

[Reuse definitions](../language-reference.md#reuse-definitions) gives the selection rules in full.

Next: [Datasets](datasets.md).

In the [weather family](weather.md):

- Every name in a file, imported: `use lib/stats.spit` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A file's names under an alias: `use lib/stats.spit as stats` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey.spit)
- Chosen names from a file: `use nonempty, average, what_if from lib/stats.spit as stats` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- An imported check: `check: stats::nonempty` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
