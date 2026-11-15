# The weather family

The weather family is four versions of one small analysis of a weather network: sites in regions, with a temperature file for each day. Each version adds to the one before, and together they use every feature of the language, so every page of this guide can point to a concrete case. They are in [`examples/runnable/weather/`](https://github.com/eclnz/spit/tree/dev/examples/runnable/weather), they need only `awk`, `sort` and Python, and each one runs with `spit-bash`. Each version has a folder of its own, and `shared/` holds the library, script and tools file that more than one uses.

## The versions

- [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit) finds each site's climate, the mean of its daily readings, and it names its own dataset, so it needs no recipe.
- [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit) removes each sensor's calibration offset first, and compares each site with its region. It adds types, stages, checks, a `verify`, resources, and a library of shared definitions.
- [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit) asks what each site's mean would be under each warmer or cooler scenario. It adds broadcasting, an operation carried out by steps, files that travel beside other files, folders, including one that starts as a copy of another and a directory view, standard streams, tools and the keyword blocks.
- [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit) reads an untidy survey, with its recipe [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). It adds the recipe's rules to exclude and require data, discovery by directory, and a person's review.

The first three read the tidy dataset in `network/`, and the fourth reads `survey/`, which has missing days, a site that has not reported, a stray file and a file of verdicts. Each version writes to its own folder under `output/`.

<!-- spit-doc: fragment -->

```sh
cd examples/runnable/weather
spit dag v3_scenarios.spitin --counts
spit-bash run v3_scenarios.spitin -j 4
```

Two inventories show the `.spitout` form: [v1_two_sites.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_two_sites.spitout) is written by hand, in the nested form, and [v4_survey.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitout) is what `spit inputs` writes for the survey, with its removals and verdicts.

## How the features are checked

[features.json](https://github.com/eclnz/spit/blob/dev/examples/checks/features.json) lists each feature below with a pattern that a version must match. `examples/checks/check.py` fails when a feature is missing from its file, when a SPIT keyword belongs to no feature, when a guide section named here does not exist, or when this page leaves a feature out. A feature that the language gains therefore has to gain a case here before it is merged.

## Artifacts

Where the artifacts are, what they are called, and the files that travel together. The section of the guide that explains each feature is [Artifacts](artifacts.md).

- A source with a type, an extension and dimensions: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [identity](artifacts.md#identity).
- A source with no type: `source reading .csv [region, site, day]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [identity](artifacts.md#identity).
- A source with no dimensions: `source title : Label .txt` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [identity](artifacts.md#identity).
- The order of the dimensions: `dimensions [region, site, day, scenario]` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [dimension order](artifacts.md#dimension-order).
- A declared type with parameters: `type Readings<Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [types](artifacts.md#types).
- A type with an argument: `operation clean(days: many Readings<Raw> @min(2), offset: Offset) -> Readings<Clean> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [types](artifacts.md#types).
- A one-letter type variable: `operation thin(series: Readings<K>) -> Readings<K> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [types](artifacts.md#types).
- A longer type variable: `Readings<$Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [types](artifacts.md#types).
- Strict typing: `types strict` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [types](artifacts.md#types).
- Standard typing: `types standard` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [types](artifacts.md#types).
- A path rule for a source: `path reading: {region}/{site}/{day}.csv` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [paths](artifacts.md#paths).
- A path rule for an output: `path climate: climates/{region}/{site}.txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [paths](artifacts.md#paths).
- A default path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [paths](artifacts.md#paths).
- How the dimensions are spelled in a path: `default entities: {@key}={@value} separated "_"` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [placeholders](artifacts.md#placeholders).
- A different label for a dimension in paths: `entities site: place` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](artifacts.md#placeholders).
- A default extension: `default ext: .txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [extensions](artifacts.md#extensions).
- An optional part of a path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [optional parts](artifacts.md#optional-parts).
- The stage in a path: `default path: {@stage}/{@product}[/{@entities}]` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [placeholders](artifacts.md#placeholders).
- A shape on a source placeholder: `reading: {region}/{site}/{day:date}.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [reading dimensions from files](artifacts.md#reading-dimensions-from-files).
- A source beside another source: `location_note : Text .txt beside location` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [source sidecars](artifacts.md#source-sidecars).
- An output beside an input: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [companion outputs](artifacts.md#companion-outputs).
- A suffix that is appended to the whole name: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [companion outputs](artifacts.md#companion-outputs).
- An output beside another output: `"_summary.txt" beside report` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [companion outputs](artifacts.md#companion-outputs).
- A folder as an output: `operation bundle(means: many Mean) -> Bundle /` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [folders](artifacts.md#folders).
- An output folder that starts as a copy of an input: `operation catalogue(bundle: Bundle) -> Catalogue / from bundle` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [seeded folders](artifacts.md#seeded-folders).
- A folder of files other jobs write, read as one input: `operation pack(mean: Text, card: Text) -> Pack / view` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [directory views](artifacts.md#directory-views).
- An output's folder and name in a command: `command write: python3 {@here}/report.py {board} {report.dir} {report.stem}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [companion outputs](artifacts.md#companion-outputs).
- A hand-written inventory in the nested form: `[day=2025-01-05,2025-01-09,2025-01-13]:` in [v1_two_sites.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_two_sites.spitout). See [inventories](datasets.md#inventories).
- An inventory written by `spit inputs`: `source_paths:` in [v4_survey.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitout). See [inventories](datasets.md#inventories).

## Operations

What a job runs, what it checks, and what it needs. The section of the guide that explains each feature is [Operations](operations.md).

- An operation: `operation day_mean(day) -> Mean` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [signatures](operations.md#signatures).
- A port that takes a collection: `operation site_mean(means: many Mean) -> Mean` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [signatures](operations.md#signatures).
- A bound on a collection: `operation clean(days: many Readings<Raw> @min(2), offset: Offset) -> Readings<Clean> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [signatures](operations.md#signatures).
- An operation with named outputs: `-> (mean: Mean, spread: Mean` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [signatures](operations.md#signatures).
- A step that assigns several products: `site_mean, site_spread = summarise(thinned)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [signatures](operations.md#signatures).
- A command: `command day_mean:` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [command templates](operations.md#command-templates).
- A command written over several lines: `clean: awk -F, -v site={site} -v off=@scalar(offset)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [command templates](operations.md#command-templates).
- Several commands in one job: `order:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [several commands in one job](operations.md#several-commands-in-one-job).
- A command that reads standard input and writes standard output: `count: wc -l < {series} > {@output}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [standard input and output](operations.md#standard-input-and-output).
- A command that writes standard output: `> {@output}` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [standard input and output](operations.md#standard-input-and-output).
- A job's own value in a command: `-v site={site}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [placeholders](operations.md#placeholders).
- Each artifact's value of a dimension: `{means.site}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](operations.md#placeholders).
- A collection as one argument: `@join(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](operations.md#placeholders).
- A collection as a file of paths: `@list(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](operations.md#placeholders).
- A number read from a file: `@scalar(offset)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [placeholders](operations.md#placeholders).
- The cores a job was given: `command league: sort -g -r --parallel={@cores} -o {@output} {means}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [resources](operations.md#resources).
- The resources a job needs: `resources league: cores=2, memory=64M` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [resources](operations.md#resources).
- A script beside the pipeline as a source: `tabulate : Script .py here` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](operations.md#placeholders).
- A script found with the pipeline's folder: `command write: python3 {@here}/report.py {board} {report.dir} {report.stem}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [placeholders](operations.md#placeholders).
- A check on one artifact: `check rows(n): sh -c 'test "$(wc -l < "$1")" -eq "$2"' sh {@path} {n}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [artifact checks](operations.md#artifact-checks).
- A check with no parameter: `check nonempty: test -s {@path}` in [stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/stats.spit). See [artifact checks](operations.md#artifact-checks).
- A check on a source: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [artifact checks](operations.md#artifact-checks).
- A check on an input port: `series: Readings<$Kind> @check(nonempty)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [artifact checks](operations.md#artifact-checks).
- A check on an output: `spread: Mean @check(plausible(30))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [artifact checks](operations.md#artifact-checks).
- A check every output runs: `default check: nonempty` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [artifact checks](operations.md#artifact-checks).
- A default check that an output drops: `@check(!stats::nonempty)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [artifact checks](operations.md#artifact-checks).
- A test of a job's inputs: `verify clean: test -s {offset}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [verification](operations.md#verification).
- The tools a recipe records: `tools from ../shared/network.spitenv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [tools](operations.md#tools).
- A tool's identity from a probe: `python: probe python3 --version` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv). See [tools](operations.md#tools).
- A tool's identity as fixed text: `awk: system-awk` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv). See [tools](operations.md#tools).
- A tool declared in the pipeline: `tool:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [tools](operations.md#tools).
- The tools an operation depends on: `write: python, !awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [tools](operations.md#tools).
- The tools every operation depends on: `uses: awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [tools](operations.md#tools).
- An operation whose output stores its paths: `anchored:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [tools](operations.md#tools).

## Pipelines

How steps connect, and how definitions are shared. The section of the guide that explains each feature is [Pipelines](pipelines.md).

- A step that gathers a dimension: `climate = site_mean(day_means @vary(day))` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [collections](pipelines.md#collections).
- A step that gathers several dimensions: `ranking = league(site_mean @vary(region, site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [collections](pipelines.md#collections).
- A step that broadcasts a dimension: `shift @each(scenario)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [broadcasting](pipelines.md#broadcasting).
- An input that pins values: `against_dune = compare(site_mean, site_mean @where(region=coast, site=dune))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [matching](pipelines.md#matching).
- An input matched on one dimension: `against_region = compare(site_mean, region_mean @same(region))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [matching](pipelines.md#matching).
- A step whose output is described: `region_mean : Mean [region] = average(site_mean @vary(site))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [steps](pipelines.md#steps).
- A step written over several lines: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [steps](pipelines.md#steps).
- A stage: `stage clean:` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [stages](pipelines.md#stages).
- A stage inside a stage: `stage site:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [stages](pipelines.md#stages).
- An operation carried out by steps: `operation what_if(script: Script, mean: Mean, shift: Shift) -> (result: Mean):` in [stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/stats.spit). See [composite operations](pipelines.md#composite-operations).
- A call to an imported composite operation: `outcome = stats::what_if(` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [composite operations](pipelines.md#composite-operations).
- Every name in a file, imported: `use ../shared/stats.spit` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit). See [imports](pipelines.md#imports).
- A file's names under an alias: `use ../shared/stats.spit as stats` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [imports](pipelines.md#imports).
- Chosen names from a file: `use nonempty, average, what_if from ../shared/stats.spit as stats` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [imports](pipelines.md#imports).
- An imported check: `check: stats::nonempty` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [imports](pipelines.md#imports).
- A dataset named by the pipeline: `root ../network` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [syntax](pipelines.md#syntax).
- An output root named by the pipeline: `output ../output/v1` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit). See [syntax](pipelines.md#syntax).
- A block of sources: `source:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of path rules: `path:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of checks: `check:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of defaults: `default:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of commands: `command:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of resources: `resources:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of tools used: `uses:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit). See [syntax](pipelines.md#syntax).
- A block of operations: `operation:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [syntax](pipelines.md#syntax).
- A block of verifications: `verify:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [syntax](pipelines.md#syntax).
- A block of reviews: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [syntax](pipelines.md#syntax).

## Datasets

What a recipe says about one dataset, and how inventories record it. The section of the guide that explains each feature is [Datasets](datasets.md).

- A recipe's pipeline, root and output: `pipeline v4_survey.spit` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [recipes](datasets.md#recipes).
- A recipe that inherits the pipeline's root: `pipeline v1_means.spit` in [v1_means.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spitin). See [recipes](datasets.md#recipes).
- Source paths in a recipe: `inputs:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [source paths](datasets.md#source-paths).
- A path for every source without one: `default: alerts/{alert}.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [source paths](datasets.md#source-paths).
- Groups from directories: `contexts: {region}/{site}` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [contexts](datasets.md#contexts).
- Excluding one artifact: `exclude reading[region=valley,site=sedge,day=2025-01-17]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- Excluding a group by value: `exclude [region=highland]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- Excluding groups with too few: `[region, site] where reading count<2` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- Excluding groups that have a value: `[region, site] where reading has day=2025-01-21` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- Excluding groups that lack a value: `[region, site] where reading missing day=2025-01-09` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- A block of exclusions: `exclude:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
- Requiring a count: `require [region, site] where calibration count=1` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [coverage requirements](datasets.md#coverage-requirements).
- Requiring a value: `[region, site] where reading has day=2025-01-05` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [coverage requirements](datasets.md#coverage-requirements).
- Requiring a count of contexts: `[region] where contexts[region,site] count>=2` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [coverage requirements](datasets.md#coverage-requirements).
- A block of requirements: `require:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [coverage requirements](datasets.md#coverage-requirements).
- A review declared: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [human review](datasets.md#human-review).
- A review of an output: `clean(days: many Readings @min(2), offset: Offset) -> Readings .csv @review(looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [human review](datasets.md#human-review).
- A review of every output: `default review: looked_at` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [human review](datasets.md#human-review).
- An output a review does not judge: `total(counts: many Count) -> Count @review(!looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit). See [human review](datasets.md#human-review).
- A file of verdicts: `review looked_at from ../survey/qc/looked_at.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [human review](datasets.md#human-review).
- A recipe with a reason beside an exclusion: `]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). See [exclusions](datasets.md#exclusions).
