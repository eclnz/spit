# Datasets

A pipeline says what to make. A dataset is the set of files it is made from. This page covers how to point a pipeline at a dataset, how SPIT finds the files, and how to leave data out, require data, or wait for a person's judgement. The examples come from the [weather network](walkthrough.md), the other [showcase analyses](https://github.com/eclnz/spit-showcase), and the cohort and review examples in the repository's `examples/` folder.

## Recipes

A `.spitin` recipe selects a pipeline and identifies the data for one run. The spoken-digits analysis reads the Free Spoken Digit Dataset from a folder called `data`, and says what a complete dataset looks like:

<!-- spit-doc: recipe
--- digits.spit
source recording : Wav .wav [speaker, digit, take]
-->

```spit
pipeline digits.spit
root data

inputs:
    recording: recordings/{digit:digits}_{speaker}_{take:digits}.wav

# Every speaker says every digit.
require [speaker] where recording count>=10
```

Both paths are relative to the recipe file. `pipeline` names the pipeline and `root` names the dataset folder. A recipe's source paths, exclusions and requirements apply to one dataset only, so the same pipeline can have a recipe for each.

- A root is declared once, in the recipe or in the pipeline. Omit it from the recipe to inherit the pipeline's, and note that declaring it in both is an error.
- `output results` gives generated files a root of their own: see [output root](#output-root).
- A pipeline that declares its own `root` and source paths needs no recipe. `spit dag pipeline.spit` plans it directly. A pipeline with no root can take `--root data` on the command line.
- Selection rules always belong in a recipe, however short: `exclude`, `require`, `review ... from` and `tools from`. A line in the wrong file is an error that says which file it belongs in.

`spit check dataset.spitin` checks the recipe's rules against the pipeline without reading any data. `spit inputs dataset.spitin` scans the dataset and settles the recipe's rules, and `spit dag dataset.spitin` plans the jobs.

In the [weather family](weather.md):

- A recipe's pipeline, root and output: `pipeline v4_survey.spit` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A recipe that inherits the pipeline's root: `pipeline v1_means.spit` in [v1_means.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spitin)

### Source paths

A source's path rule can be in the pipeline, when every dataset has the same layout, or in the recipe's `inputs:` block, when the layout belongs to one dataset. The terrain analysis keeps its settings with the data, so every one is a source with a path:

<!-- spit-doc: recipe
--- terrain.spit
source tile : Dem .tif [region, x, y]
source grid : Options .opt [region]
source ramp : Ramp .txt
source row : Python .py
-->

```spit
pipeline terrain.spit
root .

inputs:
    tile: data/tiles/{region}/{x}_{y}.tif
    grid: grids/{region}.opt
    ramp: ramp.txt
    row: row.py
```

A source takes the first rule it has of these: its own `path` rule from either file, the recipe's `default:` entry inside `inputs:`, and then the pipeline's `default path:`. Defining the same source's path in both files is an error. `spit check dataset.spitin --path-rules` shows which rule each product takes. An `inputs:` entry may also be `default: rawdata/sub-{sub}/{@product}`, which finds every source that has no rule of its own.

In the [weather family](weather.md):

- Source paths in a recipe: `inputs:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A path for every source without one: `default: alerts/{alert}.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

### Output root

Generated files go under the dataset root unless the recipe or pipeline names an `output` folder. A separate output root keeps the dataset as it was delivered, which may be read-only, and keeps two analyses of it apart. The weather recipe reads `data/raw` and writes to `data/results`:

<!-- spit-doc: recipe
--- weather.spit
source reading : Readings .csv [region, site, day]
-->

```spit
pipeline weather.spit
root data/raw
output data/results

inputs:
    reading: {region}/{site}/{day}.csv
```

Commands run from `data/results`, so a job names the dataset's files from there, as `../raw/coast/dune/site.json`. The folder need not exist yet, because the runner makes it. A pipeline run without a recipe can take `--output-root results` instead.

## Discovery

SPIT finds source files by matching each source's path rule against the files under the root. `spit inputs` prints what it found, and `spit dag` notes how many artifacts it counted. For the weather network:

```text
note: found 1485 source artifacts under `data/raw`
```

A rule matches a file's whole path, so `{region}/{site}/{day}.csv` does not match `coast/dune/2025-06-26.csv.bak`. Check the count first: a rule that matches too much or too little is the most common reason for missing jobs.

- A **shape** narrows a placeholder. `{digit:digits}_{speaker}_{take:digits}` reads only digits as the digit and the take, so a stray `notes_theo_x.wav` is not a recording. See [reading dimensions from files](artifacts.md#reading-dimensions-from-files).
- **Unmatched files** are counted in a note. `spit inputs dataset.spitin --unmatched` lists them instead of writing an inventory. If one site's file were named `Site.json` instead of `site.json`, it would be listed: `coast/dune/Site.json`.
- Values are compared as written. `site=Dune` and `site=dune` are different sites, and `day=3` and `day=03` are different days. SPIT notes values that differ only in letter case or leading zeros, since they are often one file filed twice.
- Folders whose names start with a `.`, such as `.git` or a runner's state, are not scanned.
- Links to files and folders are followed.

### Contexts

An `inputs:` entry named `contexts:` records groups from directories, even empty ones, so a rule can notice that a group has no files. The cohort example records every subject-session folder:

<!-- spit-doc: output
spit inputs examples/patterns/cohort/cohort.spitin
-->

```text
sources:
    t1w[sub=01,ses=01]
    t1w[sub=01,ses=02]
    t1w[sub=02,ses=01]
    t1w[sub=02,ses=02]
    bold[sub=01,ses=01,run=1]
    bold[sub=01,ses=01,run=2]
    bold[sub=01,ses=02,run=1]
    bold[sub=01,ses=02,run=2]
    bold[sub=02,ses=01,run=1]
    bold[sub=02,ses=01,run=2]
    bold[sub=02,ses=02,run=1]

contexts:
    [sub=01,ses=01]
    [sub=01,ses=02]
    [sub=02,ses=01]
    [sub=02,ses=02]
```

The recipe holds `contexts: sub-{sub}/ses-{ses}`, so each `[sub,ses]` folder is a group SPIT has seen on disk. Subject 03 has one session, so it has a group but too few T1 images, and the next section removes it. A directory that is absent supplies no group, and contexts never create combinations of values: SPIT keeps only the groups found on disk. A context entry neither requires files nor rejects files outside it. Write a `require` rule for every obligation.

In the [weather family](weather.md):

- Groups from directories: `contexts: {region}/{site}` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

## Exclusions

`exclude` leaves data out for a reason the recipe records. The files stay on disk, and a comment on the line is kept as the reason. The weather recipe leaves out two days at one site, where a sensor lay in grass cuttings:

<!-- spit-doc: recipe
--- weather.spit
source reading : Readings .csv [region, site, day]
-->

```spit
pipeline weather.spit
root data/raw

inputs:
    reading: {region}/{site}/{day}.csv

exclude reading[region=valley,site=sedge,day=2025-04-15]    # buried in cuttings
exclude reading[region=valley,site=sedge,day=2025-04-19]    # buried in cuttings
```

SPIT reports each removal on standard error, and records it in the inventory:

```text
note: excluded reading[region=valley,site=sedge,day=2025-04-15] (line 14): buried in cuttings
note: excluded reading[region=valley,site=sedge,day=2025-04-19] (line 15): buried in cuttings
```

There are three ways to name what to remove:

- `exclude bold[sub=02,ses=02,run=2]` removes one artifact. A source with all its dimensions names one artifact, and one with some of them names part of the source, so `exclude bold[sub=02]` removes every BOLD run of subject 2.
- `exclude [site=sedge]` removes a whole group: every artifact and context with `site=sedge`, from every source.
- `exclude [sub] where t1w count<2` removes every group that meets a condition, as the data changes. After `where` comes a source, or `contexts[...]`, and then a count such as `count<2`, a `has run=3` test, or a `missing run=1,2` test.

The cohort recipe uses the first and third:

```text
note: excluded bold[sub=02,ses=02,run=2] (line 8): motion spike
note: excluded [sub=03] by `exclude [sub] where t1w count<2` (line 7); found 1
```

An exclusion that matches nothing is an error, so a typo or a rule the data has outgrown does not pass unnoticed. A rule that would remove every group is an error.

In the [weather family](weather.md):

- Excluding one artifact: `exclude reading[region=valley,site=sedge,day=2025-01-17]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Excluding a group by value: `exclude [region=highland]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Excluding groups with too few: `[region, site] where reading count<2` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Excluding groups that have a value: `[region, site] where reading has day=2025-01-21` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Excluding groups that lack a value: `[region, site] where reading missing day=2025-01-09` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A block of exclusions: `exclude:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A recipe with a reason beside an exclusion: `]` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

## Coverage requirements

`require` fails when the data that remains is incomplete. The weather recipe demands that every site has exactly one calibration and one location, and the digits recipe demands that every speaker said every digit:

<!-- spit-doc: recipe
--- weather.spit
source location : Site .json [region, site]
source calibration : Calibration .json [region, site]
-->

```spit
pipeline weather.spit
root data/raw

inputs:
    location: {region}/{site}/site.json
    calibration: {region}/{site}/calibration.json

require [region, site] where calibration count=1
require [region, site] where location count=1
```

A failed requirement stops planning and names the group. With a site's location file misspelt, the weather recipe stops:

```text
note: 1 file under `data/raw` matches no source rule and is not read: `coast/dune/Site.json`; `spit inputs weather.spitin --unmatched` lists it
error: source coverage for `location` at [region=coast,site=dune]: expected exactly 1 artifact(s), found 0
```

The note beside it points at the cause. SPIT applies the rules in a fixed order, whatever order they are written in: named exclusions first, then conditional exclusions, and `require` last, on what remains. A `require` that finds no group at all is an error, since a check of nothing is not a pass. A rule can count with `count=1`, `count>=2` and the other comparisons, or demand values with `has`, as in `require [sub, ses] where bold has run=1,2`.

| To | Write |
| --- | --- |
| Remove a known artifact or group | `exclude bold[sub=02,ses=02,run=2]` |
| Remove every group that meets a condition | `exclude [sub] where t1w count<2` |
| Stop when data is incomplete | `require [region, site] where calibration count=1` |
| Plan what can be made despite missing data | `spit dag --partial`, a choice for one planning run |

In the [weather family](weather.md):

- Requiring a count: `require [region, site] where calibration count=1` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Requiring a value: `[region, site] where reading has day=2025-01-05` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- Requiring a count of contexts: `[region] where contexts[region,site] count>=2` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A block of requirements: `require:` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

## Human review

An `exclude` states a reason the recipe knows. A **review** is a judgement a person makes about an artifact, such as looking at a scan and finding it unusable. The pipeline declares what a review judges, and the recipe names the file that holds the verdicts. In the review example, a person looks at each plot's tidied list before it is counted:

<!-- spit-doc: setup
source list : Lines [plot]
operation count(list: Lines) -> Count
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
-->

```spit
review looked_at

operation tidy(list: Lines) -> Lines @review(looked_at)
command tidy: sort -u {list} > {@output}
tidied = tidy(list)
counted = count(tidied)
```

`@review(looked_at)` goes where `@check(...)` goes: on a source, an input port or an output. On a source or an output it judges every artifact of that product for every step that reads it, and `default review: looked_at` judges every output in a file or stage. In the recipe, `review looked_at from qc/looked_at.csv` names the verdict file, relative to the recipe:

```text
plot,verdict,reason
garden,pass,
orchard,fail,figs are not plums
```

- A row gives a `pass` or `fail` to the artifacts whose dimensions it names. An empty cell leaves its dimension out, so one row covers every value of it.
- An artifact that passes is used as usual.
- An artifact that fails, or that nobody has judged yet, is **withheld** from the steps the review covers. A review with no file yet holds back everything it judges.
- A job that reads a withheld artifact through a single input is left out. A job with a `many` input uses the members that remain, and `@min` counts those.
- The job that makes a reviewed output still runs, so the pictures and summaries a person judges can be made from every artifact.

Withheld artifacts are not incomplete, so `spit dag` succeeds and needs no `--partial`. It notes each one. Here three plots are tidied, so a person can look at them, and only the one that passed is counted:

<!-- spit-doc: output
spit dag examples/runnable/review/review.spitin --counts
-->

```text
jobs  step
   3  tidied = tidy
   1  counted = count
   1  total = gather
   5  total
note: review `looked_at` holds back tidied[plot=allotment]: no verdict yet
note: review `looked_at` rejected tidied[plot=orchard] (qc/looked_at.csv line 3): figs are not plums
```

`garden` passed, `orchard` failed with a reason, and `allotment` has no verdict yet. A verdict file may cover more than a plan uses: a row that matches no artifact is a note, not an error. A review judges and removes nothing, and the verdicts are recorded in the plan so that a runner can say what is waiting for one. [Reviews](../language-reference.md#reviews) gives the file format and its errors.

In the [weather family](weather.md):

- A review declared: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A review of an output: `clean(days: many Readings @min(2), offset: Offset) -> Readings .csv @review(looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A review of every output: `default review: looked_at` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- An output a review does not judge: `total(counts: many Count) -> Count @review(!looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A file of verdicts: `review looked_at from ../survey/qc/looked_at.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

## Inventories

An inventory, a `.spitout`, is the record of a dataset's settled inputs. `spit inputs` writes one, and a person or a dataset indexer can write one too:

<!-- spit-doc: fragment -->

```sh
spit inputs dataset.spitin -o dataset.spitout
spit dag pipeline.spit dataset.spitout
```

An inventory has sections, each optional:

- `sources:` lists the observed artifact identities.
- `contexts:` lists groups, with or without files.
- `source_paths:` carries source path rules that came from a recipe, so the plan needs no recipe.
- `removed:` and `verdicts:` record what exclusions removed and what reviews judged.
- `root` and `output` come first, when `spit inputs -o` knows where the dataset is.

The cohort example's inventory records what its recipe removed, with each rule, where it is, and the reason:

<!-- spit-doc: output
spit inputs examples/patterns/cohort/cohort.spitin
-->

```text
removed:
    bold[sub=02,ses=02,run=2]
        rule: exclude bold[sub=02,ses=02,run=2]
        at: line 8
        reason: motion spike
    [sub=03]
        rule: exclude [sub] where t1w count<2
        at: line 7
        found: 1
```

A hand-written inventory can be as short as the identities:

<!-- spit-doc: spitout -->

```text
sources:
    reading[region=coast,site=dune,day=2025-06-26]
    reading[region=coast,site=dune,day=2025-06-30]
```

The planner also needs a path rule for `reading`, from the pipeline or `source_paths:`. An inventory with a `root` line has `dag` check that the source files exist on disk, and one without it checks none. Rescanning rewrites the `removed:` section from the recipe, so a removal survives a rescan. The [inputs reference](../language-reference.md#inputs) gives the whole format.

Next: [Plans](plans.md).

In the [weather family](weather.md):

- A hand-written inventory in the nested form: `[day=2025-01-05,2025-01-09,2025-01-13]:` in [v1_two_sites.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_two_sites.spitout)
- An inventory written by `spit inputs`: `source_paths:` in [v4_survey.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitout)
