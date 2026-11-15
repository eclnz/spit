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

Here `terrain` is one file for the whole network, and `location` is one file for each site. The type is optional too, so `source calibration` alone declares one untyped artifact. A source can give the extension its files have after its type, as `.json` above, and a `/` there makes its artifacts [folders](#folders).

In the [weather family](weather.md):

- A source with a type, an extension and dimensions: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A source with no type: `source reading .csv [region, site, day]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A source with no dimensions: `source title : Label .txt` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

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

- The order of the dimensions: `dimensions [region, site, day, scenario]` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

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

A type describes a kind of file, and SPIT never opens a file to confirm it. To test contents, use a [check or `verify`](operations.md#checks-and-verification) in the operation.

Variables belong in operation signatures, not on products. A single capital letter such as `S` is a variable, and so is a longer name with `$`, such as `$Kind` or `$SourceSpace`. Any other capitalised name, such as `World`, is a concrete type.

In the [weather family](weather.md):

- A type with an argument: `operation clean(days: many Readings<Raw> @min(2), offset: Offset) -> Readings<Clean> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A one-letter type variable: `operation thin(series: Readings<K>) -> Readings<K> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A longer type variable: `Readings<$Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### Declared types

`type` declares a type with parameters, and how many arguments it takes wherever it is written:

<!-- spit-doc: fragment -->

```spit
type Frame<Kind, Space>
source scan : Frame<Diffusion> [subject]
```

<!-- spit-doc: output
spit check wrong.spit || true
--- wrong.spit = @previous
-->

```text
error: line 2, column 8: source `scan`: `Frame<Diffusion>` has 1 type argument, but `Frame` is declared `type Frame<Kind, Space>` and takes 2 type arguments
```

- Arguments are given by position, and a nested type, as in `Frame<Diffusion,Space<Sub01,T1>>`, is checked in turn.
- A type that is not declared is not checked, unless the pipeline is strict.
- A type with no parameters, such as `Script`, needs no declaration, and `type Script` is an error that says to remove the line.
- Declaring a type again with the same parameters is one declaration; with others, it is an error. An import brings the declarations of the file it reads.

In the [weather family](weather.md):

- A declared type with parameters: `type Readings<Kind>` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### Strict typing

`types strict`, once at the top of the pipeline, makes every missing type an error. Once SPIT has inferred what it can:

- each source has a type with no unknown part;
- each port of an operation a step uses has a type, which may hold variables;
- each product a step makes has a type, written or inferred;
- each type with arguments is declared.

A missing type is reported once, where it is missing, with the line to write, as ``error: line 8, column 8: `types strict` needs a type on source `atlas`, as in `source atlas : Type` ``. `types standard`, the default, checks only the types that are known. The importing file's `types` line decides for everything it imports, and a recipe takes none.

In the [weather family](weather.md):

- Strict typing: `types strict` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- Standard typing: `types standard` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

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

`default path:` is the rule for every product that has none of its own, and is how outputs are placed. A rule for one product, `path t1w: ...` in the pipeline or an entry in the recipe's `inputs:`, takes precedence. SPIT never invents a path: an output with no rule is an error, and a source needs a rule from the pipeline, a recipe or an inventory. A source with no dimensions takes a fixed path, as `path testset: eval/testset.parquet`.

A path is relative and names a file: it does not end in `/`, and has no empty, `.` or `..` folder. SPIT checks every rule when it reads the pipeline, before any data, and its errors say what to change: a dimension the product does not have, one left out, or a rule that puts a file inside another's. Write a source's rule beside its `source` line and an output's beside its step.

In the [weather family](weather.md):

- A path rule for a source: `path reading: {region}/{site}/{day}.csv` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A path rule for an output: `path climate: climates/{region}/{site}.txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A default path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)

### Placeholders

| Placeholder | Value |
| --- | --- |
| `{sub}`, `{region}`, `{site}` | A dimension's value |
| `{@product}` | The product's name |
| `{@entities}` | Every dimension, in pipeline order, spelled by `default entities:` |
| `{@stage}` | The name of the stage the step is in |

SPIT's own placeholders start with `@`, and a plain name is a dimension, so a dimension may be called `product` or `stage`. An imported product's `{@product}` is written `alias.name`. A product made outside every stage has no `{@stage}`, and a product with no dimensions has no `{@entities}`, so a rule that covers one puts them in an [optional part](#optional-parts).

In the [weather family](weather.md):

- The stage in a path: `default path: {@stage}/{@product}[/{@entities}]` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### Entity spelling

`{@entities}` has no spelling of its own. `default entities:` gives one:

```spit
default entities: {@key}-{@value} separated "_"
default entities sub: subject
```

- The template holds `{@key}` and then `{@value}`, once each, and `separated` gives the text between dimensions. This one writes `sub=01` and `ses=2` as `subject-01_ses-2`.
- `default entities sub: subject` changes only how `sub` is written in paths. Its identity is still `sub`, and the dimensions keep their [order](#dimension-order).
- The text around and between them stays in one file name: no spaces, `/`, `\`, `%`, braces or brackets. It needs punctuation other than `-`, which values may hold, so that SPIT can read the dimensions back from a path. `{@key}_{@value} separated "-"` and `{@key}-{@value} separated "_"` both work.
- The nearest spelling wins: a stage's covers its products in place of the file's. A recipe's spells the rules the recipe gives. A rule that uses `{@entities}` with no spelling to cover it is an error.
- An imported source keeps its library's spelling, or takes the caller's if its library has none. What imported operations make takes the caller's.

In the [weather family](weather.md):

- How the dimensions are spelled in a path: `default entities: {@key}={@value} separated "_"` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A different label for a dimension in paths: `entities site: place` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

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

A part is dropped when a dimension in it is absent, when `{@stage}` has no stage, or when `{@entities}` has no dimensions. Parts work in every path rule except a context pattern. They cannot nest, and each must hold a placeholder that could be absent. Write `[[` and `]]` for literal brackets.

In the [weather family](weather.md):

- An optional part of a path: `default path: {@product}[/{@entities}]` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)

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

Without the shapes, `recordings/3_theo_notes.wav` would read `notes` as the take, and with them it matches no rule and is counted with the [unmatched files](datasets.md#discovery).

- Shapes go only on rules that read sources: a source's `path`, a recipe's `inputs:` entries and `default:`, and context patterns. On a rule that writes, a shape would check nothing, and SPIT says so.
- Two placeholders side by side in a source's rule need something to tell where one ends: `{a}{b}` and `{a:digits}{b:digits}` are rejected, while `{year:year}{n:digits}` is fine because a year is four digits.
- A value can still hold the text after its placeholder: with `in/{a}-{b}.txt`, `a=x-y, b=z` reads back as `a=x, b=y-z`. `spit dag` warns of such a record; choose a separator the values do not hold.
- A file that two sources' rules both match is an error naming both, since SPIT has no precedence between rules. Shape both, or name more of the path.

In the [weather family](weather.md):

- A shape on a source placeholder: `reading: {region}/{site}/{day:date}.csv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)

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

The sources declare the extension their files have. A product that declares none, such as the weather network's `cleaned`, takes the `default ext:` of `.csv`, and one that declares another, such as a figure panel's `.png`, keeps it. An extension is a `.` and letters, digits, `-` or `_`, in one part or several. An untyped output can have one, as `-> .txt`, and a multi-output operation gives one for each port.

A product's path is its rule, with an extension added when the rule ends without one:

1. A product's own `path` rule takes the extension its operation or source declares. `default ext:` never applies to it, and without a declared extension the rule is used as written.
2. A default rule takes the declared extension, else the nearest stage's `default ext:`, else the pipeline's.

A rule's extension is the text after its last placeholder, from the first `.`. A rule that already ends with the right extension is left as it is, so a full BIDS-style path can keep it. A rule that ends with another is an error, since the tool writes a different file:

<!-- spit-doc: output
spit check p4.spit || true
--- p4.spit
default entities: {@key}={@value} separated "_"
default path: derivatives/{@product}/{@entities}
source scan : Image [sub]
path scan: raw/{sub}.nii.gz
operation align(moving: Image) -> Transform .mat
command align: align {moving} {@output}
path matrix: derivatives/matrix/{sub}.txt
matrix = align(scan)
-->

```text
path `matrix` ends in `.txt`, but operation `align` writes `.mat`; drop the extension or use `.mat`; SPIT reads the extension from the first `.` after the last placeholder, so keep `.` out of the name before it
```

So a `.` in a name after the last placeholder starts the extension: write `acq-1p5T`, not `acq-1.5T`. A [folder](#folders) takes only the extension it declares, never `default ext:`. An operation whose tool picks the format from the file name declares none, and its path rule decides. `spit check --path-rules` shows where each product's extension comes from.

In the [weather family](weather.md):

- A default extension: `default ext: .txt` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)

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

The path rule names the folder without a trailing slash. A runner makes an output folder, empty, before the job's first command runs, so `bwa index` writes `genome.amb` and its other files into it, and a later job passes `{index}/genome` as the prefix. A tool that copies a folder into its output copies the contents, as `cp -R {notes}/. {@output}` does, since `cp -R {notes} {@output}` puts `notes` inside the folder.

- A folder source is found by matching its rule against folders, and the files inside it are read with it, so they are not unmatched. A file that matches a folder source's rule, or a folder a file source's, is skipped with a warning.
- `{subject.dir}` is the folder an output folder is in, and `{subject.stem}` its name, so `recon-all -sd {subject.dir} -s {subject.stem}` works.
- A job owns the folder it writes: no other product may be written in it or read from it. A source may sit inside a source folder, since no job writes either.
- A folder is never written `beside` another file, nor has an output beside it.
- `spit dag --paths` shows a folder's path with a `/` after it.

In the [weather family](weather.md):

- A folder as an output: `operation bundle(means: many Mean) -> Bundle /` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Seeded folders

A tool that changes a folder in place, rather than writing a new one, needs a folder to start from. `from` and an input port's name after an output folder start that output as a copy of the input folder. The weather network's indexed bundle adds a count to a copy of the bundle:

<!-- spit-doc: setup
source outcome : Mean .txt [site]
default path: out/{@product}[/{site}]
operation bundle(means: many Mean) -> Bundle /
command bundle: cp {means} {@output}/
bundled = bundle(outcome @vary(site))
-->

```spit
operation catalogue(bundle: Bundle) -> Catalogue / from bundle
command catalogue: sh -c 'echo "$(ls "$1" | wc -l) outcomes" > "$1/index.txt"' sh {@output}
catalogued = catalogue(bundled)
```

The command is given only its output, `out/catalogued`, which holds a copy of `out/bundled` when the command starts, and it writes `index.txt` beside the copied files. The bundle itself is never changed, and a job that runs again starts again from a fresh copy. The input must be one folder: a folder source, or a folder another job writes, and its type may differ from the output's. A named output is written `(catalogue: Catalogue / from bundle)`. The runner copies links that stay inside the folder as links, and fails the job before its command if the folder holds a link that leads out of it.

In the [weather family](weather.md):

- An output folder that starts as a copy of an input: `operation catalogue(bundle: Bundle) -> Catalogue / from bundle` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Directory views

Some tools read a folder of files that several jobs write, such as FSL's `oxford_asl --fslanat`, which reads a `.anat` folder. A `view` output names that folder without any job writing it. Its inputs list the files it holds, and the path rule of each file puts it inside the view's folder. The weather network packs each site's mean and card together:

<!-- spit-doc: setup
source mean : Mean .txt [site]
source card : Text .txt [site]
-->

```spit
operation describe(mean: Mean) -> Text .txt
command describe: sh -c 'echo "mean $(cat "$1")" > "$2"' sh {mean} {@output}
described = describe(mean)
path described: pack/{site}/mean.txt

operation label(card: Text) -> Text .txt
command label: cp {card} {@output}
labelled = label(card)
path labelled: pack/{site}/card.txt

operation pack(mean: Text, card: Text) -> Pack / view
packed = pack(described, labelled)
path packed: pack/{site}

operation read_pack(pack: Pack) -> Text .txt
command read_pack: sh -c 'cat "$1/mean.txt" "$1/card.txt" > "$2"' sh {pack} {@output}
read = read_pack(packed)
path read: summaries/{site}.txt
```

For the site `dune`, `read_pack` is given one folder, `pack/dune`, and waits for both files in it. A view has no command and makes no job, so `spit dag --jobs` lists `describe`, `label` and `read_pack` only. Each of a view's files must be written by a job, have the view's dimensions, and lie inside its folder, and no other product may write into it. A changed file reruns the jobs that read the view. A file in the folder that the view does not list is not tracked, so list every file the tool relies on. A view takes no checks; check its files where they are made. Each file is matched on all the view's dimensions, so a site that has one file and lacks another has no view.

In the [weather family](weather.md):

- A folder of files other jobs write, read as one input: `operation pack(mean: Text, card: Text) -> Pack / view` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Source sidecars

Files that travel together often share a name and differ by extension. Declare the main file as a source, and each companion `beside` it. The drone-survey example has a photo with GPS, IMU and metadata sidecars:

```spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
path raw_photo: site-{site}/visit-{visit}/photos/site-{site}_visit-{visit}_shot-{shot}_photo.raw
source photo_gps : GpsTrack .gpx beside raw_photo
source photo_imu : ImuTrace .imu beside raw_photo
source photo_json : CaptureMetadata .json beside raw_photo
```

A photo at `.../shot-3_photo.raw` has its sidecars at `.../shot-3_photo.gpx`, `.imu` and `.json`. Each companion takes its dimensions and path from the main source, so it has no path rule of its own, and a step reads it by name.

- Declare the main source first. It is a file, and a companion cannot be the main source of another.
- An extension such as `.json` replaces the main file's, which it must then have. A quoted suffix such as `"_mask.nii.gz"` follows the name without its extension. A `+` before either appends it to the whole name: `source reference_fai : Fai +.fai beside reference` reads `genome.fasta.fai`.
- A recipe can give the main source's path, with its extension, in place of the pipeline, and the `.spitout` records only the main source's.
- If an identity has some of the files and lacks others, SPIT warns, as `warning: raw_photo[site=A,visit=2,shot=3] has .raw and .gpx but no .json`. It fails only a job that reads the missing one, and `dag --partial` plans the rest. A file an `exclude` rule removed is not counted as missing.

In the [weather family](weather.md):

- A source beside another source: `location_note : Text .txt beside location` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

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

With the BAM at `out/sorted/sample=A.bam`, the index is `out/sorted/sample=A.bam.bai`, in the same folder. A suffix replaces the sibling's extension, and a `+` before it appends the suffix to the whole file name instead, as index files are named. A suffix is an extension or quoted text of letters, digits, `.`, `-` and `_`, as `mask: Image "_mask.nii.gz" beside brain`.

- A `beside` output has no path rule of its own, and the command need not name it, since the tool is not told where to write it.
- It names a single-artifact input of the operation, or another output that is not itself beside another. With a suffix that replaces an extension, the file it follows must have one.
- It is not optional: if the tool does not write it, the job fails. A flag that turns the file off, such as `dcm2niix -b n`, needs an operation that does not declare it, and a file a tool writes only for some data, as `dcm2niix` writes `.bval` only for diffusion, needs two operations.

An output can also be `beside` one of the operation's inputs, for a tool that writes next to the file it reads. `samtools faidx` writes a FASTA's index beside the FASTA, in the dataset, where `bcftools` looks for it:

<!-- spit-doc: setup
source reference : Fasta .fasta
-->

```spit
operation faidx(fasta: Fasta) -> Fai +.fai beside fasta
command faidx: samtools faidx {fasta}
genome_fai = faidx(reference)
```

With the reference at `reference/genome.fasta`, `genome_fai` is `reference/genome.fasta.fai`. A step that needs the index names it as an input, so its jobs wait for the one that writes it, though its command need not name it. The file follows its input wherever that input's path comes from, and stays with the source under the dataset root. The input is a file with the step's dimensions, and a single-artifact input.

Next: [Operations](operations.md).

In the [weather family](weather.md):

- An output beside an input: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A suffix that is appended to the whole name: `operation index(series: Readings) -> Index +.idx beside series` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- An output beside another output: `"_summary.txt" beside report` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- An output's folder and name in a command: `command write: python3 {@here}/report.py {board} {report.dir} {report.stem}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
