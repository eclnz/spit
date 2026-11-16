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

- A root is declared once, in the recipe or in the pipeline. Omit it from the recipe to inherit the pipeline's, still relative to the pipeline's folder, and note that declaring it in both is an error. It may use `..` or be absolute, and `root .` is the recipe's own folder. An import never brings a library's root, and `spit check` warns when the folder is not there.
- `output results` gives generated files a root of their own: see [output root](#output-root).
- A pipeline that declares its own `root` and source paths needs no recipe. `spit dag pipeline.spit` plans it directly. A pipeline with no root can take `--root data` on the command line.
- Selection rules always belong in a recipe, however short: `exclude`, `require`, `review ... from` and `tools from`. A line in the wrong file is an error that says which file it belongs in.

`spit check dataset.spitin` checks the recipe's rules against the pipeline without reading any data. `spit inputs dataset.spitin` scans the dataset and settles the recipe's rules, and `spit dag dataset.spitin` plans the jobs. A recipe is given alone: the pipeline it names is not named again on the command line.

In the [weather family](weather.md):

- A recipe's pipeline, root and output: `pipeline v4_survey.spit` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A recipe that inherits the pipeline's root: `pipeline v1_means.spit` in [v1_means.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spitin)

### Which file a line belongs in

A pipeline is the work, for any dataset. A recipe binds it to one dataset: where the folder is, where the sources are when the pipeline does not say, and what to leave out or require. Selection always goes in the recipe, even a single `exclude` line:

| Line | Pipeline | Recipe |
| --- | --- | --- |
| `source`, `dimensions`, `types`, `type`, `operation`, `command`, `verify`, `resources`, `tool`, `uses`, `anchored`, `check`, `review name`, steps, `stage`, `use`, `default ext:`, `default check:`, `default review:` | yes | no |
| `path product:` for a product a step makes | yes | no |
| `path product:` for a source | either, not both | either, not both |
| `default path:` | covers outputs, and sources nothing else covers | covers sources only |
| `root`, `output` | either, not both | either, not both; otherwise the pipeline's |
| `pipeline`, `tools from`, `inputs:`, `require`, `exclude`, `review name from file` | no | yes |

Put a source's rule in the pipeline when every dataset has the same layout, and in the recipe when the layout is this dataset's. A line in the wrong file is an error that says where it belongs:

<!-- spit-doc: output
spit check step.spitin || true
--- survey.spit
source image : Image [sub]
path image: data/{sub}.nii.gz
--- step.spitin
pipeline survey.spit
root data
smoothed = smooth(image)
-->

```text
error: line 3, column 1: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
```

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

A source takes the first of these rules it has:

1. its own `path` rule, from either file;
2. the recipe's `default:` entry inside `inputs:`, such as `default: rawdata/sub-{sub}/{@product}`;
3. the pipeline's `default path:`.

Defining the same source's path in both files is an error. `spit check dataset.spitin --path-rules` shows which rule each product takes, and marks those the recipe gives `(recipe)`. An output's path never comes from the recipe.

An entry may hold several rules, one on each line beneath the source's name, as `t1:` does in the [messy export](https://github.com/eclnz/spit/blob/dev/examples/patterns/messy/messy.spitin). See [skipping text and fixing values](artifacts.md#skipping-text-and-fixing-values). `default:` holds one rule.

A default finds only the sources it can spell. One that names `{@stage}` outside an optional part finds no source, since no source is made in a stage, and one that names `sub` outside a part finds no source without `sub`. Each source adds the extension it declares, so `default: rawdata/sub-{sub}/{@product}` finds both `rawdata/sub-01/t1w.nii.gz` and `rawdata/sub-01/events.tsv`.

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

- The folder is relative to the file that names it, and may be absolute. A pipeline or a recipe names it, not both.
- Every file a job writes goes under it, except a file written [beside a source](artifacts.md#companion-outputs), which stays with the source.
- An output root inside the dataset, as with `root .` and `output results`, is left out of the scan, so earlier results are neither sources nor unmatched files.

## Discovery

SPIT finds source files by matching each source's path rule against the files under the root. `spit inputs` prints what it found, and `spit dag` notes how many artifacts it counted. For the weather network:

```text
note: found 1485 source artifacts under `data/raw`
```

A rule matches a file's whole path, so `{region}/{site}/{day}.csv` does not match `coast/dune/2025-06-26.csv.bak`. Check the count first: a rule that matches too much or too little is the most common reason for missing jobs.

- A **shape** narrows a placeholder. `{digit:digits}_{speaker}_{take:digits}` reads only digits as the digit and the take, so a stray `notes_theo_x.wav` is not a recording. See [reading dimensions from files](artifacts.md#reading-dimensions-from-files).
- **Unmatched files** are counted in a note. `spit inputs dataset.spitin --unmatched` lists them instead of writing an inventory. If one site's file were named `Site.json` instead of `site.json`, it would be listed: `coast/dune/Site.json`.
- Values are compared as written. `site=Dune` and `site=dune` are different sites, and `day=3` and `day=03` are different days. SPIT notes values that differ only in letter case or leading zeros, since they are often one file filed twice.
- Folders whose names start with a `.`, such as `.git` or a runner's state, are not scanned, unless a rule writes the folder's name out.
- The unmatched count leaves out SPIT's own files and anything at a path the pipeline gives its outputs, so an earlier run's results are not counted.
- A value is read only as SPIT would write it, so a file such as `in/%41.txt` is skipped with a warning.
- Links to files and folders are followed.
- Two files that read as one identity, which a `*` or several rules can give, stop planning with an error naming each. Every such identity is listed in one run. Choose among them with [`prefer path`](#two-paths-one-identity), or remove the copies with an [`exclude path`](#exclusions).

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

- A rule counts contexts as `contexts[sub,ses]`, and there must be a context entry with exactly those dimensions. Each folder counts once within its group.
- Several context entries add up, and a folder two of them find counts once.
- A context pattern names a folder relative to the root, with no `{@...}` placeholders, optional parts, `.` or `..`, or trailing `/`.

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

There are four ways to name what to remove:

- `exclude bold[sub=02,ses=02,run=2]` removes one artifact. A source with all its dimensions names one artifact, and one with some of them names part of the source, so `exclude bold[sub=02]` removes every BOLD run of subject 2.
- `exclude [site=sedge]` removes a whole group: every artifact and context with `site=sedge`, from every source.
- `exclude [sub] where t1w count<2` removes every group that meets a condition, as the data changes. After `where` comes a source, or `contexts[...]`, and then a count such as `count<2`, a `has run=3` test, or a `missing run=1,2` test.
- `exclude path ProjA_Case_4_A` removes the source file at a path under the root, or every source file in the folder there. `*` stands for any text within one name, as in `exclude path ProjA_Case_*`, and `**` is a literal star. It removes what it covers whether or not another copy is left, so for copies, [`prefer path`](#two-paths-one-identity) is safer.

An export that holds subject 4 twice, under two spellings, stops planning until the recipe keeps one:

<!-- spit-doc: output
spit inputs examples/patterns/messy/messy.spitin
-->

```text
note: excluded t1[cohort=patient,id=4,visit=A] (line 9): the scanner sent subject 4 twice
```

Without its `exclude path` and `prefer path` lines, the recipe stops:

```text
error: 2 folders read as t1[cohort=ctrl,id=8,visit=none]:
    Proj_A_Ctrl_8/Proj A - 81/anat_T1_2
    Proj_A_Ctrl_8/Proj_A_Ctrl_8/anat_T1_2
2 folders read as t1[cohort=patient,id=4,visit=A]:
    ProjA_Case_4_A/Proj A - 1234/anat_T1_9
    Proj_A_4_A/Proj A - 1234/anat_T1_9
  keep one of each with `exclude path <path>` in the recipe
```

### Two paths, one identity

`prefer path` chooses among the paths that read as one identity, and leaves every other identity alone. The messy export has control 8's T1 twice: in its session folder, named with an accession number, and sent again in a folder of its own. Its last line keeps the first:

<!-- spit-doc: recipe
--- messy.spit
source t1 : Series/ [cohort, id, visit]
-->

```spit
pipeline messy.spit
root export

inputs:
    t1:
        *_{id:digits}_{visit}{cohort=patient}/*/anat_T1*
        *_Ctrl_{id:digits}{cohort=ctrl}{visit=none}/*/anat_T1*

exclude path ProjA_Case_4_A    # the scanner sent subject 4 twice
prefer path */* - *            # a T1 sent again in a folder of its own is a copy
```

<!-- spit-doc: output
spit inputs examples/patterns/messy/messy.spitin
-->

```text
note: excluded t1[cohort=ctrl,id=8,visit=none] by `prefer path */* - *` (line 10): a T1 sent again in a folder of its own is a copy
```

- Of the paths of one identity, it keeps those its path covers, written with `*` as `exclude path` is. An identity read from one path is never removed, so a broad pattern cannot drop the only copy of a scan.
- A rule that covers all of an identity's paths, or none of them, leaves the choice to the next. Several `prefer` lines apply in the order written, after every `exclude`.
- An identity still read from more than one path is the error above. A `prefer` that chooses for no identity is an error too, as an exclusion that matches nothing is.
- What it drops is reported and recorded in the inventory with its reason, as an exclusion is.

In the [weather family](weather.md):

- Choosing among paths that read as one identity: `prefer path */*/calibration.txt` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

The cohort recipe uses the first and third ways:

```text
note: excluded bold[sub=02,ses=02,run=2] (line 8): motion spike
note: excluded [sub=03] by `exclude [sub] where t1w count<2` (line 7); found 1
```

An exclusion that matches nothing is an error, naming any value it comes close to, so a typo or a rule the data has outgrown does not pass unnoticed. A rule that would remove every group is an error.

- Removing a group removes every artifact and context within it, of every source. An artifact without all the group's dimensions, such as a subject's reference when only sessions are removed, stays.
- A conditional rule's values name a dimension within each group: `exclude [store] where sales missing store=s07` is an error, since each group has one store. Write `exclude [store=s07]`.
- A file that a context expects but lacks counts as absent, so `exclude [sub, ses] where t1w count=0` removes a session whose T1 is missing.
- Each conditional rule has one condition, and every rule is judged against the same data, so their order changes nothing.
- Values are compared as written, so a group removed as `s07` keeps a file filed as `S07`. SPIT notes values that differ only in letter case, and `dag` notes the file no job uses. Name it in a rule of its own.

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

The note beside it points at the cause. SPIT applies the rules in a fixed order, whatever order they are written in: named exclusions first, then conditional exclusions, and `require` last, on what remains. A `require` that finds no group at all is an error, since a check of nothing is not a pass. A rule can count with `count=1`, `count!=1`, `count>=2`, `count<=2`, `count>2` or `count<2`, or demand values with `has`, as in `require [sub, ses] where bold has run=1,2`. `require [] where lut count=1` checks a single shared file.

Groups come first, then `where`, then what to count and the condition. Rules that count form their groups from every artifact and context in the dataset, so a store with sales but no price list is a group whose price-list count is 0.

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

`garden` passed, `orchard` failed with a reason, and `allotment` has no verdict yet. A review judges and removes nothing, and the verdicts are recorded in the plan so that a runner can say what is waiting for one.

- **Where it is attached.** On a source or an output, a review judges that product for every step that reads it. On an input port, it judges only what that port reads. A step's product takes none: attach it to the operation's output.
- **Declaring.** A review has a name and nothing else: `review looked_at: ...` is an error. Reviews are global, as checks are, and an imported one takes the import's prefix, as `@review(lib::looked_at)`. `default review:` adds up and drops with `!` as [default checks](operations.md#default-checks) do.

In the [weather family](weather.md):

- A review declared: `review:` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A review of an output: `clean(days: many Readings @min(2), offset: Offset) -> Readings .csv @review(looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A review of every output: `default review: looked_at` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- An output a review does not judge: `total(counts: many Count) -> Count @review(!looked_at)` in [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit)
- A file of verdicts: `review looked_at from ../survey/qc/looked_at.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

### Verdict files

A verdict file is a CSV, quoted as spreadsheets write it, named by the recipe relative to the recipe's folder. One file belongs to each review.

- It has a column for each dimension it judges by, which must be a dimension of every product the review judges, a `verdict` column of `pass` or `fail`, and an optional `reason` column.
- A row judges each artifact with the values it gives. An empty cell leaves that dimension out, so in a file with `sub`, `ses`, `verdict` and `reason`, `04,,fail,withdrew` fails every session of subject 04.
- Two rows that give one artifact different verdicts are an error, as are an unknown column, a verdict that is not `pass` or `fail`, and a row with no value. Each names the file and line.
- A row that matches no artifact is a note, since a sheet often covers more than one plan: ``note: 12 verdicts in qc/looked_at.csv match no artifact (lines 6–17)``.
- A review whose file is missing, or holds no rows, has no verdicts yet. That is a note, not an error, and everything it judges is held.

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

The planner also needs a path rule for `reading`, from the pipeline or `source_paths:`, which gives each record's file.

A source whose rules skip text with `*`, fix a value or come several to a source gives no path from an identity, so each of its records names the file SPIT found after a `:`. The path runs to the end of the line, spaces and `#` included, and one of the source's rules must read it back as the record:

<!-- spit-doc: output
spit inputs examples/patterns/messy/messy.spitin
-->

```text
source_paths:
    t1:
        *_{id:digits}_{visit}{cohort=patient}/*/anat_T1*
        *_Ctrl_{id:digits}{cohort=ctrl}{visit=none}/*/anat_T1*

sources:
    t1[cohort=ctrl,id=6,visit=none]: Proj_A_Ctrl_6/Proj A - 77/anat_T1_3
    t1[cohort=ctrl,id=8,visit=none]: Proj_A_Ctrl_8/Proj A - 81/anat_T1_2
    t1[cohort=patient,id=4,visit=A]: Proj_A_4_A/Proj A - 1234/anat_T1_9
    t1[cohort=patient,id=6,visit=B]: ProjA_Case_6_B/ProjA_6/anat_T1_6
```

A record of any other source names no file, since its rule gives one.

A hand-written inventory may nest identities under a group. A list of values in a nested group expands for every value, so this is two runs of each DWI source:

<!-- spit-doc: spitout -->

```text
sources:
    source_lut
contexts:
    [sub=01,ses=01]:
        reverse_b0, t1w
        [run=01,02]:
            raw_dwi, dwi_bvec, dwi_bval
```

- `root` and `output` are relative to the `.spitout`'s folder, and come before every section, once. An inventory with a `root` line has `dag` check that the source files exist on disk, and one without it checks none. A printed inventory has neither, since where it will be kept is unknown.
- `removed:` is a record, not a rule: the records above it already leave the artifacts out. Rescanning rewrites it from the recipe, so a removal survives a rescan, and `dag` copies it into the plan.
- `verdicts:` lists each review's file, with `(missing)` for one not there yet, and each row with its verdict, file and line. The plan applies them, so a plan made from the inventory uses the same verdicts.

Next: [Plans](plans.md).

In the [weather family](weather.md):

- A hand-written inventory in the nested form: `[day=2025-01-05,2025-01-09,2025-01-13]:` in [v1_two_sites.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_two_sites.spitout)
- An inventory written by `spit inputs`: `source_paths:` in [v4_survey.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitout)
