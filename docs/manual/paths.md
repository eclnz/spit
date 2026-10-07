# Paths

Path templates bind artifact identities to relative file or folder paths. Product identity and file naming are separate: changing a path rule does not change which artifacts match a call.

## Paths

```text
path: results/{@product}/{@entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default; without one, outputs go to `out/{@product}/{@entities}`, which `spit check --path-rules` lists as `built-in default`. `path image:` overrides it for `image`. Sources never take the built-in path: a source with no rule needs one from a recipe or a `.spitout`'s `source_paths:`. A recipe's `path:` sets the default for sources instead; see [Recipes](recipe.md#recipes). Each output of a multi-output step has its own product, so its own rule. A `path:` line inside a [stage](pipeline.md#stages) sets the default for that stage's products. Paths are relative to the dataset root: a recipe must name it with `root`, an inventory may record it, or a pipeline-only scan takes it from `--root`. `--root` cannot be combined with a recipe or inventory. See [root selection](cli.md#root-selection).

A source with no dimensions can use a fixed path, such as `path testset: eval/testset.parquet`.

A template fills these placeholders from the artifact it names, here `aligned[subject=A,run=2]` made in stage `preprocess/align`:

| Placeholder | Expands to | Example |
| --- | --- | --- |
| `{@product}` | The product's name; an imported `alias::name` becomes `alias.name` | `aligned` |
| `{@entities}` | Every dimension as `dim=value`, in the pipeline's dimension order, joined by `__`; `global` for a product with no dimensions | `subject=A__run=2` |
| `{@stage}` | The stage whose block holds the step, one directory per level; an error for a product made outside every stage | `preprocess/align` |
| `{@labels}` | Every dimension as `key-value`, in pipeline dimension order, joined by `_`; no value for a product without dimensions | `subject-A_run-2` |
| `{subject}`, `{run}`, … | The value of a dimension the product declares | `A`, `2` |

SPIT's own placeholders take `@`, and a dimension takes none, so a dimension may be called `product` or `stage`. `{product}` with no such dimension is an error that says to write `{@product}`. Values keep letters, digits, and `-`; any other byte is written as `%` and two hex digits, so a value never adds a directory. `{@labels}` uses the dimension names as keys; use explicit text such as `sub-{subject}` when a dataset calls a dimension by another name. SPIT warns if a value written through `{@labels}` contains `-`, because a BIDS reader cannot recover that value from the file name.

A path may put text in `[...]` when only some products have it. SPIT keeps the group if every placeholder in it has a value for the product, or drops the whole group if a dimension is absent, `{@stage}` has no stage, or `{@labels}` has no dimensions. For example, the [cohort pipeline](https://github.com/eclnz/spit/tree/dev/examples) uses one default for run images, session averages, and subject averages:

```text
path: derivatives/sub-{sub}[/ses-{ses}][/{@stage}]/{@labels}_{@product}
ext: .nii.gz
```

For `long[sub=01]` outside every stage, that becomes `derivatives/sub-01/sub-01_long.nii.gz`; for `mc[sub=01,ses=01,run=2]` in `func`, it becomes `derivatives/sub-01/ses-01/func/sub-01_ses-01_run-2_mc.nii.gz`. A product with no dimensions can use `[{@labels}_]{@product}`. Groups work in every path rule: a default, a stage's default, a product's own rule, a source's rule in a pipeline or recipe, and the path of a source with companions, though not in a `discover` pattern, where every directory has each dimension. A group is decided per product, before any data is read, so each product has one plain template. Groups cannot nest and must contain a placeholder that could be absent; `[[` and `]]` write literal brackets. `spit check --path-rules` and `check --json` show each product's resolved template before any data is read.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare outside an optional group, a dimension no product declares even inside a group, a rule that omits one of the product's dimensions (use `{@entities}`, `{@labels}`, or name each one), two products whose rules give the same path for the same entities, such as a default rule without `{@product}`, and a rule that puts files inside another product's file path, such as `in/{id}.txt/out.txt` beside `in/{id}.txt`, or inside a [folder](#folders) a job writes. A path must be relative, name a file rather than end in `/`, and contain no empty, `.`, or `..` directory. A source no rule covers is reported by `spit check` on a recipe and by `dag`, and collisions between resolved artifact paths once jobs are bound. SPIT warns when two artifacts' paths differ only in letter case, such as `id=A` and `id=a`: where case is ignored, as by default on macOS and Windows, they are one file.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{@output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

### Extensions

A tool decides the format of the file it writes, so an operation can say which extension each output's file has, after its type. A multi-output operation gives one per port:

```text
operation align(moving: Image, reference: Image) -> Transform .mat
operation fit(runs: many Data @ min(2)) -> (weights: Weights .npz, quality: Metrics .json)
```

An extension is a `.` and letters, digits, `-` or `_`, and may have several parts, as `.nii.gz` does. An untyped output may have one too: `-> .txt`. A source declares the extension of the files it reads in the same place, after its type: `source events : Events .tsv [sub]`.

`ext:` sets the extension for operations that declare none, at the top level or in a stage, as `path:` sets the default path. Default path rules are then written without an extension:

```text
path: derivatives/{@product}/{@entities}
ext: .img
```

A product's path is its rule, completed with an extension when the rule ends without one:

1. Its own `path product:` rule takes the operation's extension, or the one a source declares, if there is one. `ext:` never applies to it, and without such an extension the rule is used as written. A recipe's default counts as each source's own rule.
2. A default rule, its stage's or the pipeline's, takes the operation's extension or the source's, else the nearest stage's `ext:`, else the pipeline's. A source with no declared extension whose files the pipeline's default rule finds takes `ext:` too.

A rule's extension is the text of its last file name after its final placeholder, from the first `.`, as `.nii.gz` in `sub-{sub}_T1w.nii.gz`. A rule that ends with the extension it would be given is left as it is, so a full BIDS-style path can keep it. A rule that ends with another is an error, since the tool writes a different file from the one the rule names:

```text
path `matrix` ends in `.txt`, but operation `align` writes `.mat`; drop the extension or use `.mat`; SPIT reads the extension from the first `.` after the last placeholder, so keep `.` out of the name before it
```

A `.` in a name after the last placeholder is read as the start of the extension too, so `out/{sub}_acq-1.5T` for an operation that writes `.csv` is this error, with `.5T` as the extension. Keep `.` out of names, as `acq-1p5T`, which takes `.csv`; a rule that writes the whole name with its extension, as `out/{sub}_acq-1.5T.csv`, is left as it is. An extension may hold digits and more than one `.`, as `.7z` and `.tar.gz` do.

A default rule that ends with an extension while an operation, a source, or `ext:` gives its products another is the same error, said once for the rule. For a source the message says the source declares the extension, as in ``path `events` ends in `.csv`, but source `events` declares `.tsv` ``.

A [folder](#folders) takes only the extension it declares, never `ext:`.

Extensions are optional. An operation whose tool picks the format from the output's name, such as a converter, declares none, and its path rule decides. A pipeline with no extensions and no `ext:` line resolves its paths as written.

`spit check --path-rules` shows each product's path with its extension, and where the extension is declared:

```text
  matrix (output): default derivatives/{@product}/{@entities}.mat, `.mat` from operation `align`
```

Path rules also find sources. With `root data`, `spit inputs recipe.spitin` lists each file under `data` whose path matches a source's rule, in the pipeline or the recipe, reading entity values from its placeholders. A rule matches a file's whole path, so `responses/{region}/wave{wave}.csv` does not match `wave3.csv.bak` or `wave3.csv.1`, and files that match no rule are left out. When a source's rule matches no file, the scan warns and names the unmatched file nearest the rule, with the text where the file and the rule part; see [Find incomplete artifacts](cli.md). To write the rules for data that already exists, `spit inputs --suggest` prints a rule for each group of files no rule matches; see [Start from the files](https://github.com/eclnz/spit/blob/dev/README.md#start-from-the-files). Links to files and directories are followed. A value is read only as SPIT writes it, so a file such as `in/%41.txt`, whose value SPIT would write `A`, is skipped with a warning rather than listed under a path no job would use.

### Shapes on a source placeholder

A placeholder in a source rule matches any text within one folder or file name, so `logs/{server}/{date}.log` reads `logs/web1/notes.log` as `date=notes`. A shape after a `:` narrows it:

```text
path log: logs/{server}/{date:date}.log
path run: raw/sub-{sub:digits}_run-{run:digits}.csv
```

| Shape | Matches | Example |
| --- | --- | --- |
| `digits` | one or more digits; leading zeros stay in the value | `07`, `120` |
| `year` | four digits, from 1900 to 2099 | `2026` |
| `date` | `YYYY-MM-DD`, a real day in a year from 1900 to 2099 | `2026-09-01` |

With `{date:date}`, `logs/web1/notes.log`, `2026-9-1.log`, `20260901.log`, `2026-09-01-final.log` and `2026-02-30.log` match no rule, so they are not source artifacts. They are counted with the other files no rule matches, and `spit inputs --unmatched` lists them. When a source then matches no file, the warning names the nearest one: `after `logs/web1/`, the file has `notes.log` where the rule has `{date:date}.log``. The set of shapes is closed; it is not a pattern language.

Where a shape may be written:

- In a source's `path` rule, in the pipeline or a recipe's `path <source>:` line or a recipe's `path:` line, and in a `.spitout`'s `source_paths:`. A recipe's `path:` is a default for sources only.
- In a `discover ... from dirs` pattern, as `discover days: [day] from dirs data/{day:date}`.
- Not in an output's rule, nor in a pipeline or stage `path:` default, which outputs take: a shape only narrows what is read, so on a rule that writes it would check nothing, and SPIT says so. A source whose rule is a pipeline default writes its shape in a rule of its own, `path <source>: ...`, or in a recipe's `path:` default, instead. `{@entities}` and `{@labels}` take no shape either; write `sub-{sub:digits}`.

A shape reads the text of a value, which holds letters, digits and `-` only, so no shape contains `.`, `_` or `/`: `2026_09_02.log` matches no `{date}` at all, and a `.` after a placeholder still starts the extension. A dimension written twice, as `{id:digits}/{id}`, has its shape in both places; give it one shape only. Two placeholders with nothing between them may not both take a shape of any length, as `{a:digits}{b:digits}` does, since where one ends is not written; `{year:year}{n:digits}` is fine, because a year is four digits, and `{date:date}-{run:digits}` has its `-`. The check also runs on each product's path once the `[...]` groups its dimensions lack are dropped: `in/{a:digits}[-{c}]{b:digits}.txt` is fine for a product with `c`, but for one without it is `in/{a:digits}{b:digits}.txt`, and SPIT rejects it, naming the product. A shaped placeholder beside an unshaped one, `{a:digits}{b}`, is allowed and splits the way unshaped neighbours do, shortest first: `123.txt` reads `a=1`, `b=23`. A file that both a shaped rule and a general rule match, such as `{date:date}.log` and `{name}.log`, is still an error naming both sources; SPIT has no precedence between rules, so shape both or name more of the path. Shaping does not tell two sources' paths apart where they are compiled: `source a [d]`, `source b [d]` with `path a: in/{d:date}.log` and `path b: in/{d:digits}.log` is rejected, since both bind `in/d.log` for the same entities. Name the two sources' dimensions differently, as `[d]` and `[n]`, as well as shaping them. A record in a `.spitout` or an `inputs` file whose value fails its rule's shape is an error, since discovery would not read its file.

`spit inputs --suggest` writes `{date:date}` and `{year:year}` for dimensions whose values are all dates or all years.

### Files a tool writes beside another

Some tools write a second file beside the one they are told to write, such as the `.json` that dcm2niix writes next to its image, or the mask FSL's `bet -m` names after its brain image. Declare such an output `beside` the output it follows, with what its file name ends with in place of that output's extension:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image "_mask.nii.gz" beside brain)
```

Its path is its sibling's, without the sibling's extension, then the suffix: beside `out/image/sub=01.nii.gz`, `meta` is `out/image/sub=01.json`, and beside `sub-01_brain.nii.gz`, `mask` is `sub-01_brain_mask.nii.gz`. `{@product}` in the sibling's rule stays the sibling's name, since the file is beside the sibling's.

The suffix is an extension, or quoted text of letters, digits, `.`, `-` and `_`; the output's own extension is the suffix from its first `.`. A `beside` output:

- may be left out of the command, since the tool is not told where to write it;
- has no path rule of its own; its path follows the sibling's;
- names another output of the same operation, which declares an extension and is not itself written beside another.

Later steps read it as any other output, and the `.spitdag` lists it among the job's outputs. A declared output is never optional: if the tool does not write it, the job fails as if its command had failed, and the jobs that read its outputs do not run (see [missing outputs](dag.md#missing-outputs)). Whether a tool writes a sidecar usually depends on how it is run, as `dcm2niix -b n` writes no `.json`, so the same flag leaves it out of every job; an operation run that way declares no `meta`. A tool that writes a file only for some data, as dcm2niix writes `.bval` and `.bvec` only for a diffusion series, is two operations, each called on the sources it fits.

### Folders

Some tools read or write a folder of files rather than one file: a DICOM series, a FreeSurfer subject, a Zarr store. A `/` after a product's type, where an extension goes, makes its artifacts folders. It may follow an extension, as in `.zarr/`:

```text
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
operation recon(t1: Image) -> (subject: FsSubject /)
operation store(table: Table) -> Zarr .zarr/
```

A folder's path rule names the folder, without a trailing `/`. `spit inputs` finds a folder source by matching its rule against the folders under the root, and the files inside a folder it finds are read with it, so they are not listed among the files no rule matches. A file whose path matches a folder source's rule, or a folder whose path matches a file source's, is skipped with a warning that says which kind the source reads. A folder source whose rule matches no folder is warned about as a file source is, naming the nearest folder rather than the nearest file. `dag` checks that each source folder exists, as it does each source file.

A command is given a folder by its path, as a file is: `{dicom}` above is `dicom/sub=01`. `{subject.dir}` is the folder it is in, and `{subject.stem}` its name without its extension, which for a folder without one is its whole name, so a tool that takes a parent folder and a name can be given both:

```text
command recon: recon-all -i {t1} -sd {subject.dir} -s {subject.stem}
```

For `subject[sub=01]`, at `out/subject/sub=01`, this passes `-sd out/subject -s sub=01`. `spit dag --paths` shows a folder's path with a `/` after it, `spit check --path-rules` names it `(source folder)` or `(output folder)`, and the `.spitdag` gives each artifact a [`kind`](dag.md#artifact).

A job owns the folder it writes, so nothing else may be written in it or read from it: a path rule that puts another product's files inside a folder a job writes is an error, as is one that puts an output inside a source folder. A source may sit in a source folder, such as `dicom/sub={sub}/info.json` beside the `dicom` folder above, since no job writes either. A folder is not written [`beside`](#files-a-tool-writes-beside-another) another output, nor has an output beside it, and a source declared [`beside`](#sidecar-files) another is a file.

### Sidecar files

Files that travel together often share a name and differ by extension. Declare the main file as a source, then declare each companion `beside` it:

```spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
path raw_photo: site-{site}/visit-{visit}/photos/shot-{shot}.raw
source photo_gps : GpsTrack .gpx beside raw_photo
source photo_json : CaptureMetadata .json beside raw_photo
```

The companion is an ordinary source that a step reads by name. It inherits `raw_photo`'s dimensions and path stem: `photo_gps` above reads `site-{site}/visit-{visit}/photos/shot-{shot}.gpx`. Declare the main source first. It must be a file with an extension; a companion cannot itself be the main source of another companion. A companion has no dimensions or path rule of its own. An extension such as `.json` replaces the main file's extension; a quoted suffix such as `"_mask.nii.gz"` is appended to its stem.

The path can come from a [recipe](recipe.md#recipes) when the layout varies by dataset. Name the main source and give its complete file path, including its extension:

```spit
# survey.spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
source photo_json : CaptureMetadata .json beside raw_photo

# dataset.spitin
pipeline survey.spit
root data
path raw_photo: site-{site}/visit-{visit}/photos/shot-{shot}.raw
```

A recipe's default `path:` also places the main source. For `path: data/{site}/{visit}/{shot}/{@product}`, the files are `data/a/1/3/raw_photo.raw` and `data/a/1/3/raw_photo.json`. The `.spitout` records the main source's path under `source_paths:`; the companion's path is derived from it. When importing definitions, import the main source to bring all its companions. With an alias, `path text::raw_photo:` names the imported main source.

When `spit inputs` scans a dataset, or a command reads records from a recipe or `.spitout`, it warns about each identity that holds some of these sources and lacks others, as `warning: raw_photo[site=A,visit=2,shot=3] has .raw and .gpx but no .json`. Warnings follow source declaration order, then value order. A file a named or conditional `exclude` rule removes is not counted as missing, nor is one listed under `.spitout`'s `removed:` section.

A missing companion is a warning because it matters only to a step that reads it. If `brain = strip(anat)` reads only the image, it can still plan every subject; a step reading `anat_meta` fails for a subject missing its JSON file. SPIT never runs a job with an input left out. `dag --partial` plans the remaining jobs and lists the omitted ones with their reasons. A recipe may instead remove a whole subject with `exclude [sub] where anat_meta count=0`.
