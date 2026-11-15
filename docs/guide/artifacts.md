# Artifacts

An **artifact** is one file or folder that a pipeline reads or makes. This page covers how artifacts are named, what types they have, where they live on disk, and the kinds of file SPIT understands.

## Identity

A **product** names a family of artifacts, such as `shard`, `sorted` or `merged`. An artifact is one member of that family, and its identity is the product name together with its **dimension** values:

| Identity | Where it comes from |
| --- | --- |
| `shard[group=alpha,part=01]` | An input file in the dataset |
| `sorted[group=alpha,part=01]` | The output of one sort job |
| `merged[group=alpha]` | The output of one merge job |

A source declares its dimensions in brackets. A step's output takes the dimensions of the input that drives it, so `sorted` has `group` and `part` because `shard` does. A step that gathers artifacts with `@vary(part)` drops `part` from its output, and one that broadcasts with `@each(scenario)` adds `scenario`. [Pipelines](pipelines.md) covers both.

A source with no dimensions is a single artifact, and takes no brackets:

<!-- spit-doc: setup
operation scale(input: Table, factor: Table) -> Table
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
source table : Table [sheet]
source calibration : Table
scaled = scale(table, calibration)
```

Every `scale` job reads the same `calibration` artifact, because a dimensionless input matches every job.

### Dimension order

A pipeline has one dimension order. It decides how a collection's artifacts are sorted and how `{@entities}` and printed identities list their dimensions. Each source states the order of its own dimensions, `[group, part]` putting `group` before `part`, and those statements together set the pipeline's order. Two sources that order a pair differently are an error.

When a product holds two dimensions that no source orders together, declare the order once:

<!-- spit-doc: setup
source runs : Run [model, config, seed]
-->

```spit
dimensions [model, config, seed]
```

`spit check` asks for the line when it needs one. [Broadcasting](pipelines.md#broadcasting) is where this usually happens.

## Types

A type says what kind of file a product holds, so that SPIT can catch a step wired to the wrong input. Types are optional. `Lines` is a plain type name, and `Frame<Native>` is a type with an argument, here naming the space a frame is in.

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
type Frame<Space>
source scan : Frame<Native> [subject]
operation smooth(frame: Frame<S>) -> Frame<S>
smoothed = smooth(scan)
```

`type Frame<Space>` declares that `Frame` takes one argument. In `smooth`'s signature, the single capital letter `S` is a variable. Each call binds it from its inputs, so `smooth(scan)` binds `S` to `Native` and `smoothed` is a `Frame<Native>`. Nothing writes that type down: SPIT infers it, and hovering over `smoothed` in the editor shows it.

A mismatch SPIT can see is an error. Connecting a `Lines` product to the `frame` port stops the check:

<!-- spit-doc: output
spit check mismatch.spit || true
--- mismatch.spit
type Frame<Space>
source notes : Lines [subject]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation smooth(frame: Frame<S>) -> Frame<S>
smoothed = smooth(notes)
-->

```text
error: line 6, column 19: type mismatch at `smooth.frame`: product `notes` is Lines, expected Frame<$S>
```

A missing type is not an error. Put types where a wrong connection would be costly. To require them everywhere, write `types strict` at the top of the pipeline: every source, port and step then needs a type, but a type SPIT can infer needs no annotation.

A type describes a kind of file, and SPIT never opens a file to confirm it. To test contents, use a [check or `verify`](operations.md#checks-and-verification) in the operation.

See [optional types](../language-reference.md#optional-types) for variables longer than one letter, declared types and strict typing.

## Paths

A **path rule** turns an artifact's identity into a file or folder path. Source paths are relative to the dataset root. The paths of files that jobs write are relative to the output root when there is one, and to the dataset root otherwise (see [the output root](datasets.md#output-root)).

```spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
```

`path shard:` is a rule for one product. `default path:` is the rule for every product that has none of its own, and is how outputs are placed. SPIT never invents a path: an output with no rule is an error, and a source needs a rule from the pipeline, a recipe or an inventory.

### Placeholders

| Placeholder | Value |
| --- | --- |
| `{group}`, `{part}` | A dimension's value |
| `{@product}` | The product's name |
| `{@entities}` | Every dimension, in pipeline order, spelled by `default entities:` |
| `{@stage}` | The name of the stage the step is in |

SPIT's own placeholders start with `@`, and a plain name is a dimension. `default entities: {@key}={@value} separated "_"` writes each dimension as `key=value` and joins them with `_`, so `sorted[group=alpha,part=01]` has the path `output/sorted/group=alpha_part=01.txt`. A rule that uses `{@entities}` without a spelling to cover it is an error. You can choose another spelling, such as BIDS-style names:

```spit
default entities: {@key}-{@value} separated "_"
default entities sub: subject
default path: derivatives/{@product}/{@entities}.nii.gz
```

This writes `derivatives/image/subject-01_ses-1.nii.gz`. `default entities sub: subject` changes only how `sub` is written in paths. Its identity is still `sub`.

### Optional parts

Square brackets mark a part of the path that appears only when every placeholder inside it has a value:

<!-- spit-doc: setup
source image : Image [sub, ses]
default entities: {@key}-{@value} separated "_"
-->

```spit
default path: derivatives/sub-{sub}[/ses-{ses}]/{@entities}_{@product}
```

For a product with no `ses`, the `/ses-{ses}` group is left out. Use the same idea for a default that must cover dimensionless products as well: in `output/{@product}[/{@entities}].txt`, the slash and the entities are dropped for a product with no dimensions. Each product still gets one fixed template, decided before any data is read.

### Which rule applies

A product-specific `path` rule wins over a default. Among defaults, the nearest wins: a stage's `default path:` is used for that stage's products in place of the pipeline's. `spit check --path-rules` lists the rule each product takes and where it came from:

<!-- spit-doc: output
spit check lines.spit --path-rules
--- lines.spit = examples/runnable/lines/lines.spit
-->

```text
Product path coverage:
  shard (source): explicit input/{group}/{part}.txt
  sorted (output): default output/{@product}/group={group}_part={part}.txt
  merged (output): default output/{@product}/group={group}.txt
```

### Escaping and collisions

A value keeps letters, digits and `-`. Any other character is written as `%` and two hex digits, so a value can never add a directory or break a file name: the group `A_B` becomes `A%5FB`.

Two products may not write the same path for the same identity. A default path with no `{@product}` makes that mistake, and SPIT reports it:

<!-- spit-doc: output
spit check collide.spit || true
--- collide.spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
operation reverse(input: Lines) -> Lines
command reverse: tac {input} > {@output}
default entities: {@key}={@value} separated "_"
default path: output/{@entities}.txt
sorted = sort_lines(shard)
reversed = reverse(shard)
-->

```text
error: line 8, column 15: products `sorted` and `reversed` bind to the same path `output/group=group_part=part.txt` for the same entities; include `{@product}` or distinguish their path rules
```

Put the product name and every dimension needed to tell the artifacts apart into the rule. SPIT also warns when two paths differ only in letter case, such as `id=A` and `id=a`, because on some systems they are one file.

### Reading dimensions from files

A source path is also a pattern. `spit inputs` matches it against the files under the dataset root and reads each dimension's value from the placeholder. `input/{group}/{part}.txt` reads `input/alpha/01.txt` as `shard[group=alpha,part=01]`. A placeholder matches any text within one folder or file name, so a shape can narrow it:

<!-- spit-doc: setup
source log : Log [server, date]
source run : Run [sub, run]
-->

```spit
path log: logs/{server}/{date:date}.log
path run: raw/sub-{sub:digits}_run-{run:digits}.csv
```

| Shape | Matches |
| --- | --- |
| `digits` | One or more digits, such as `07` |
| `year` | Four digits, from 1900 to 2099 |
| `date` | A real `YYYY-MM-DD` date |

With these rules, `logs/web1/notes.log` is not a `log` artifact, because `notes` is not a date. Shapes belong only on rules that read sources. [Shapes on a source placeholder](../language-reference.md#shapes-on-a-source-placeholder) lists the limits.

## File kinds

A product can say more than its dimensions: what its file name ends with, whether it is a folder, and which other files travel with it.

### Extensions

An extension after a type completes a path rule that leaves the extension out. It can be a single suffix or several, such as `.nii.gz`:

```spit
default entities: {@key}={@value} separated "__"
default path: derived/{@product}/{@entities}
default ext: .csv
source events : Events .tsv [sub]
operation analyse(events: Events) -> Report .json
operation tally(events: Events) -> Tally
```

`events` takes `.tsv` and the `Report` takes `.json`. `Tally` declares none, so it takes the `default ext:` of `.csv`. An extension an operation declares is the one its tool writes, so a path rule that ends with a different one is an error. [Extensions](../language-reference.md#extensions) gives the full precedence.

### Folders

A `/` after the type or extension marks an artifact that is a folder:

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
operation recon(dicom: Dicom) -> FsSubject /
subject = recon(dicom)
```

The path rule names the folder without a trailing slash. SPIT checks that a folder source is a folder, and a job's output folder may not hold or overlap any other product's path. A runner makes an output folder, empty, before the job's first command runs. See [folders](../language-reference.md#folders).

### Source sidecars

Files that travel together often share a name and differ by extension. Declare the main file as a source, and each companion `beside` it:

```spit
source raw : Image .raw [site, shot]
source metadata : Json .json beside raw
path raw: photos/{site}/{shot}.raw
```

A source at `photos/a/3.raw` has its sidecar at `photos/a/3.json`. The companion takes its dimensions and path from the main source, so it has no path rule of its own. A recipe can give the main source's path instead of the pipeline (see [datasets](datasets.md#source-paths)). If an identity has a main file but no companion, SPIT warns, and fails only a job that reads the missing one.

### Companion outputs

Some tools write a second file next to the one they are told to write. Declare that file `beside` the output it follows, so SPIT knows where it is without the command naming it:

<!-- spit-doc: setup
source dicom : Dicom / [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation convert(dicom: Dicom) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}
image, meta = convert(dicom)
```

For `out/image/sub=01.nii.gz`, `meta` is `out/image/sub=01.json`. A suffix replaces the sibling's extension. Put a `+` before it to append it to the whole file name instead, as index files are named:

<!-- spit-doc: setup
source sam : Sam .sam [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai +.bai beside bam)
command sort:
    samtools sort -o {bam} {sam}
    samtools index {bam}
bam, bai = sort(sam)
```

Here `bai` is `out/bam/sample=A.bam.bai`. An output can also be `beside` one of the operation's inputs, for a tool that writes next to the file it reads, such as `samtools faidx` writing a FASTA's index. [Files a tool writes beside another](../language-reference.md#files-a-tool-writes-beside-another) shows that form.

Next: [Operations](operations.md).
