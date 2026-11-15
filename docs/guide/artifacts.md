# Artifacts

An **artifact** is one file or folder that a pipeline reads or makes. This page covers how artifacts are named, what types they have, where they live on disk, and the kinds of file SPIT understands. The examples come from the [weather network](walkthrough.md) and the other [showcase analyses](https://github.com/eclnz/spit-showcase).

## Identity

A **product** names a family of artifacts, such as `reading`, `cleaned` or `league`. An artifact is one member of that family, and its identity is the product name together with its **dimension** values:

| Identity | Where it comes from |
| --- | --- |
| `reading[region=coast,site=dune,day=2025-06-26]` | One day's file from one sensor, in the dataset |
| `cleaned[region=coast,site=dune]` | The output of one `clean` job |
| `regional[region=coast]` | The output of one regional average |
| `league` | The single ranking of every site |

A source declares its dimensions in brackets. A step's output takes the dimensions of the input that drives it, so `cleaned` has `region` and `site` because `calibration` does. A step that gathers artifacts with `@vary(day)` drops `day` from its output, and one that broadcasts with `@each(alert)` adds `alert`. [Pipelines](pipelines.md) covers both.

A product with no dimensions is a single artifact, and takes no brackets. Every job that takes it as an input reads the same file:

```spit
source terrain : Terrain .csv
source location : Site .json [region, site]
```

Here `terrain` is one file for the whole network, and `location` is one file for each site.

In the [weather family](weather.md):

- A source with a type, an extension and dimensions: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A source with no type: `source reading .csv [region, site, day]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A source with no dimensions: `source title : Label .txt` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)

### Dimension order

A pipeline has one dimension order. It decides how a collection's artifacts are sorted and how `{@entities}` and printed identities list their dimensions. Each source states the order of its own dimensions, `[region, site, day]` putting `region` before `site` before `day`, and those statements together set the pipeline's order. Two sources that order a pair differently are an error.

When a product holds two dimensions that no source orders together, declare the order once. In the weather network, `alert` comes from the threshold files and no source holds it beside `region` or `site`:

<!-- spit-doc: setup
source reading : Readings .csv [region, site, day]
source threshold : Threshold .json [alert]
-->

```spit
dimensions [region, site, day, alert]
```

`spit check` asks for the line when it needs one. A ragged sweep declares `dimensions [model, config, seed]` for the same reason, where `model` and `config` are broadcast against each other.

In the [weather family](weather.md):

- The order of the dimensions: `dimensions [region, site, day, scenario]` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

## Types

A type says what kind of file a product holds, so that SPIT can catch a step wired to the wrong input. Types are optional. `Site` is a plain type name, and `Readings<Raw>` is a type with an argument. The weather network uses the argument to follow a series through its stages: `Readings<Raw>` as read, `Readings<Clean>` after calibration, and `Readings<SeaLevel>` after the effect of height is removed.

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation reduce(series: Readings<Clean>) -> Readings<SeaLevel>
reduced = reduce(cleaned)

operation average(series: many Readings<S>) -> Readings<S>
regional = average(cleaned @vary(site))
```

In `average`'s signature, the single capital letter `S` is a variable. Each call binds it from its inputs, so `average(cleaned @vary(site))` binds `S` to `Clean` and `regional` is a `Readings<Clean>`. Nothing writes that type down: SPIT infers it, and hovering over `regional` in the editor shows it. One operation then serves readings of any kind, and passes the kind to its output.

A mismatch SPIT can see is an error. Comparing a site's reduced series with a series that was not reduced stops the check, before anything runs:

<!-- spit-doc: output
spit check mismatch.spit || true
--- mismatch.spit
source cleaned : Readings<Clean> [region, site]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation reduce(series: Readings<Clean>) -> Readings<SeaLevel>
reduced = reduce(cleaned)
operation compare(readings: Readings<SeaLevel>, baseline: Readings<SeaLevel>) -> Anomaly
anomaly = compare(reduced, cleaned)
-->

```text
error: line 7, column 28: type mismatch at `compare.baseline`: product `cleaned` is Readings<Clean>, expected Readings<SeaLevel>
```

A missing type is not an error. Put types where a wrong connection would be costly. To require them everywhere, write `types strict` at the top of the pipeline: every source, port and step then needs a type, but a type SPIT can infer needs no annotation.

A type describes a kind of file, and SPIT never opens a file to confirm it. To test contents, use a [check or `verify`](operations.md#checks-and-verification) in the operation. Declared types, longer variable names and strict typing are in [optional types](../language-reference.md#optional-types).

In the [weather family](weather.md):

- A declared type with parameters: `type Readings<Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A type with an argument: `operation clean(days: many Readings<Raw> @min(2), offset: Offset) -> Readings<Clean> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A one-letter type variable: `operation thin(series: Readings<K>) -> Readings<K> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- A longer type variable: `Readings<$Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- Strict typing: `types strict` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)
- Standard typing: `types standard` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

## Paths

A **path rule** turns an artifact's identity into a file or folder path. Source paths are relative to the dataset root. The paths of files that jobs write are relative to the output root when there is one, and to the dataset root otherwise (see [the output root](datasets.md#output-root)).

The MRI quality-control pipeline gives its outputs one default, and the recipe gives its source one rule:

```spit
source t1w : T1w .nii.gz [sub]
default entities: {@key}-{@value} separated "_"
default path: out/{@stage}/{@product}[/{@entities}]
```

<!-- spit-doc: recipe
--- t1w_qc.spit
source t1w : T1w .nii.gz [sub]
-->

```spit
pipeline t1w_qc.spit
root data

inputs:
    t1w: sub-{sub}/anat/sub-{sub}_T1w.nii.gz
```

`default path:` is the rule for every product that has none of its own, and is how outputs are placed. A rule for one product, `path t1w: ...` in the pipeline or an entry in the recipe's `inputs:`, takes precedence. SPIT never invents a path: an output with no rule is an error, and a source needs a rule from the pipeline, a recipe or an inventory.

In the [weather family](weather.md):

- A path rule for a source: `path reading: {region}/{site}/{day}.csv` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A path rule for an output: `path climate: climates/{region}/{site}.txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A default path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)

### Placeholders

| Placeholder | Value |
| --- | --- |
| `{sub}`, `{region}`, `{site}` | A dimension's value |
| `{@product}` | The product's name |
| `{@entities}` | Every dimension, in pipeline order, spelled by `default entities:` |
| `{@stage}` | The name of the stage the step is in |

SPIT's own placeholders start with `@`, and a plain name is a dimension. `default entities: {@key}-{@value} separated "_"` writes each dimension as `key-value` and joins them with `_`, so an artifact with `sub=01` and `ses=2` is spelled `sub-01_ses-2`. A rule that uses `{@entities}` without a spelling to cover it is an error. `default entities sub: subject` changes only how `sub` is written in paths. Its identity is still `sub`.

In the [weather family](weather.md):

- How the dimensions are spelled in a path: `default entities: {@key}={@value} separated "_"` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)
- A different label for a dimension in paths: `entities site: place` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- The stage in a path: `default path: {@stage}/{@product}[/{@entities}]` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked.spit)

### Optional parts

Square brackets mark a part of the path that appears only when every placeholder inside it has a value. The weather network places every product it makes with one rule:

<!-- spit-doc: setup
source cleaned : Readings .csv [region, site]
source threshold : Threshold .json [alert]
source reading : Readings .csv [region, site, day]
dimensions [region, site, day, alert]
-->

```spit
default path: [{region}/][{site}/]{@product}[_{alert}][_{day}]
default ext: .csv
```

A product with `region` and `site` is `coast/dune/cleaned.csv`. A product with only `region` is `coast/regional.csv`, a product with no dimensions is `league.csv`, and `alert_days[region=coast,site=dune,alert=frost]` is `coast/dune/alert_days_frost.csv`. Each product still gets one fixed template, decided before any data is read.

In the [weather family](weather.md):

- An optional part of a path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)

### Which rule applies

A product-specific `path` rule wins over a default. Among defaults, the nearest wins: a stage's `default path:` is used for that stage's products in place of the pipeline's. `spit check --path-rules` lists the rule each product takes and where it came from. For the quality-control pipeline and its recipe:

```text
Product path coverage:
  t1w (source): explicit sub-{sub}/anat/sub-{sub}_T1w.nii.gz (recipe)
  mosaics (output): default out/inspect/mosaics/sub-{sub}.png, `.png` from operation `mosaic`
  summaries (output): default out/measure/summaries/sub-{sub}.json, `.json` from operation `summarise`
  cohort (output): default out/measure/cohort.tsv, `.tsv` from operation `table`
```

### Escaping and collisions

A value keeps letters, digits and `-`. Any other character is written as `%` and two hex digits, so a value can never add a directory or break a file name: a site called `A_B` becomes `A%5FB`.

Two products may not write the same path for the same identity. A default path with no `{@product}` makes that mistake, and SPIT reports it:

<!-- spit-doc: output
spit check collide.spit || true
--- collide.spit
source cleaned : Readings<Clean> [region, site]
default entities: {@key}={@value} separated "_"
default path: out/{@entities}.csv
operation reduce(series: Readings<Clean>) -> Readings<SeaLevel>
reduced = reduce(cleaned)
-->

```text
error: line 3, column 15: products `cleaned` and `reduced` bind to the same path `out/region=region_site=site.csv` for the same entities; include `{@product}` or distinguish their path rules
```

Put the product name and every dimension needed to tell the artifacts apart into the rule. SPIT also warns when two paths differ only in letter case, such as `site=A` and `site=a`, because on some systems they are one file.

### Reading dimensions from files

A source path is also a pattern. `spit inputs` matches it against the files under the dataset root and reads each dimension's value from the placeholder. The `{region}/{site}/{day}.csv` rule reads `coast/dune/2025-06-26.csv` as `reading[region=coast,site=dune,day=2025-06-26]`. A placeholder matches any text within one folder or file name, so a shape can narrow it. The spoken-digits analysis reads the dataset's own file names this way:

```spit
source recording : Wav .wav [speaker, digit, take]
path recording: recordings/{digit:digits}_{speaker}_{take:digits}.wav
```

`recordings/3_theo_12.wav` is `recording[digit=3,speaker=theo,take=12]`, and a file with letters where the digit should be is not a recording.

| Shape | Matches |
| --- | --- |
| `digits` | One or more digits, such as `07` |
| `year` | Four digits, from 1900 to 2099 |
| `date` | A real `YYYY-MM-DD` date, such as `2025-06-26` |

Without the shapes, `recordings/3_theo_notes.wav` would read `notes` as the take. Shapes belong only on rules that read sources. [Shapes on a source placeholder](../language-reference.md#shapes-on-a-source-placeholder) lists the limits.

In the [weather family](weather.md):

- A shape on a source placeholder: `reading: {region}/{site}/{day:date}.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey.spitin)

## File kinds

A product can say more than its dimensions: what its file name ends with, whether it is a folder, and which other files travel with it.

### Extensions

An extension after a type completes a path rule that leaves the extension out. It can be a single suffix or several, such as `.nii.gz`:

```spit
source location : Site .json [region, site]
source threshold : Threshold .json [alert]
source t1w : T1w .nii.gz [sub]
default ext: .csv
```

The sources declare the extension their files have. A product that declares none, such as the weather network's `cleaned`, takes the `default ext:` of `.csv`, and one that declares another, such as a figure panel's `.png`, keeps it. An extension an operation declares is the one its tool writes, so a path rule that ends with a different one is an error. [Extensions](../language-reference.md#extensions) gives the full precedence.

In the [weather family](weather.md):

- A default extension: `default ext: .txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means.spit)

### Folders

A `/` after the type or extension marks an artifact that is a folder. The variant-calling analysis indexes its reference genome into one:

<!-- spit-doc: setup
source reference : Fasta .fasta
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
operation bwa_index(reference: Fasta) -> BwaIndex /
command bwa_index: bwa index -p {@output}/genome {reference}
genome_index = bwa_index(reference)
```

The path rule names the folder without a trailing slash. A runner makes an output folder, empty, before the job's first command runs, so `bwa index` writes `genome.amb` and its other files into it, and a later job passes `{index}/genome` as the prefix. SPIT checks that a folder source is a folder, and a job's output folder may not hold or overlap any other product's path. See [folders](../language-reference.md#folders).

In the [weather family](weather.md):

- A folder as an output: `operation bundle(means: many Mean) -> Bundle /` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

### Source sidecars

Files that travel together often share a name and differ by extension. Declare the main file as a source, and each companion `beside` it. The drone-survey example has a photo with GPS, IMU and metadata sidecars:

```spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
path raw_photo: site-{site}/visit-{visit}/photos/site-{site}_visit-{visit}_shot-{shot}_photo.raw
source photo_gps : GpsTrack .gpx beside raw_photo
source photo_imu : ImuTrace .imu beside raw_photo
source photo_json : CaptureMetadata .json beside raw_photo
```

A photo at `.../shot-3_photo.raw` has its sidecars at `.../shot-3_photo.gpx`, `.imu` and `.json`. Each companion takes its dimensions and path from the main source, so it has no path rule of its own, and a step reads it by name. A recipe can give the main source's path instead of the pipeline. If an identity has a main file but lacks a companion, SPIT warns, and fails only a job that reads the missing one. See [sidecar files](../language-reference.md#sidecar-files).

In the [weather family](weather.md):

- A source beside another source: `location_note : Text .txt beside location` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)

### Companion outputs

Some tools write a second file next to the one they are told to write. Declare that file `beside` the output it follows, so SPIT knows where it is without the command naming it. The variant-calling analysis sorts a BAM and indexes it in one job:

<!-- spit-doc: setup
source sam : Sam .sam [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai +.bai beside bam)
command sort:
    samtools sort --threads {@cores} -o {bam} {sam}
    samtools index {bam}
sorted, sorted_bai = sort(sam)
```

With the BAM at `out/sorted/sample=A.bam`, the index is `out/sorted/sample=A.bam.bai`, in the same folder. A suffix replaces the sibling's extension, and a `+` before it appends the suffix to the whole file name instead, as index files are named.

An output can also be `beside` one of the operation's inputs, for a tool that writes next to the file it reads. `samtools faidx` writes a FASTA's index beside the FASTA, in the dataset, where `bcftools` looks for it:

<!-- spit-doc: setup
source reference : Fasta .fasta
-->

```spit
operation faidx(fasta: Fasta) -> Fai +.fai beside fasta
command faidx: samtools faidx {fasta}
genome_fai = faidx(reference)
```

With the reference at `reference/genome.fasta`, `genome_fai` is `reference/genome.fasta.fai`. A step that needs the index names it as an input, so its jobs wait for the one that writes it. [Files a tool writes beside another](../language-reference.md#files-a-tool-writes-beside-another) shows more forms.

Next: [Operations](operations.md).

In the [weather family](weather.md):

- An output beside an input: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- A suffix that is appended to the whole name: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- An output beside another output: `"_summary.txt" beside report` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
- An output's folder and name in a command: `command write: python3 {@here}/scripts/report.py {board} {report.dir} {report.stem}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios.spit)
