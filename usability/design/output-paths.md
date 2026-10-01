# Design: dataset root, output extensions and sidecars

Status: steps 1 to 4, the recipe's `root` line, extensions on operation outputs, source groups and `beside` outputs, are done. Step 5 is a proposal.

## Problem

Agents in every usability round called path rules repetitive; in [round 3](../ROUND3.md) it was, with choosing `--root`, the biggest first-build friction in the cohort task. Most of that repetition takes one of two forms.

**Overrides that only change the extension.** In [`field_survey.spit`](../../examples/commands/field_survey/field_survey.spit), all 8 derived `path x:` rules copy the default `derivatives/{product}/{entities}` to change `.img` to `.tif`, `.mat`, `.txt`, `.rows` or `.csv`. In [`mrtrix3_act.spit`](../../examples/commands/mrtrix3_act/mrtrix3_act.spit), all 8 derived rules, stage defaults included, do the same. Each copy also stops inheriting from the default: change `derivatives/` to `out/` and the 8 overrides silently keep the old directory.

**Sidecars.** `photo_gps`, `photo_imu` and `photo_json` are `raw_photo`'s path with another extension, and each repeats the full template and dimension list. On the derived side, a tool such as `dcm2niix` writes a `.json` beside its image without being given a path for it, which SPIT cannot express today: a command must use every output placeholder, and each output gets its own path rule.

A third form, BIDS derivative names whose entity labels change with each product's dimensions (`…_run-{run}_mc` beside `…_avg`), is out of scope here. See [Not covered](#not-covered).

**The dataset root.** Every path is relative to the dataset root: the recipe's folder, or `--root`. Agents kept the pipeline and recipe in their working folder and the data in `data/`, so every `inputs` and `dag` command needed `--root data`, and each agent had to work out that the flag moves every path at once. One agent first put the recipe inside `data/`, where the scan counted the recipe itself as a file matching no source rule. No one guessed wrong, but several called it the most careful part of the task.

## Overview

1. [A recipe names its dataset root](#1-a-recipe-names-its-dataset-root) with a `root` line. It is independent of the rest and the smallest change, so it lands first.

The file format is chosen by the tool, so the extension belongs on the operation that runs it, not on each product's path. The other changes build on that:

2. [Extensions on operation outputs](#2-extensions-on-operation-outputs), with a default for operations that declare none.
3. [Source groups](#3-source-groups) for sidecars in the input data.
4. [Implicit outputs](#4-implicit-outputs): `beside` for files a tool writes next to another output.
5. [Directory and stem placeholders](#5-directory-and-stem-placeholders) for tools that take a folder and a name instead of a path.

Each of 3 to 5 depends on SPIT knowing an output's extension, so they land after 2. Alongside 2, `spit check` reports each product's resolved path, so the editor can show it (see [Seeing the resolved path](#seeing-the-resolved-path)).

## 1. A recipe names its dataset root

**Syntax.** A recipe may name its dataset root once, beside its `pipeline` line:

```text
pipeline cohort.spit
root data
```

The folder is relative to the recipe's folder, like the `pipeline` line, and may use `..` or be absolute. A pipeline has no `root` line: it describes the computation, and the recipe describes one dataset.

**Meaning.** The `root` line replaces the recipe's folder wherever that is the default today: the folder a recipe scans, the base of every source and output path, the base of `discover` patterns, and where each source file must exist. So `spit dag cohort.spitin` needs no flag.

- `--root` still overrides it, for running the same recipe against a copy of the data elsewhere.
- A recipe that writes its own `sources:` records is still not scanned unless `--root` is given. The `root` line is a default, like the recipe's folder, not a request to scan.

**The recipe inside the root.** The scan leaves SPIT's own files (`.spit`, `.spitin`, `.spitout` and `.spitdag`) out of the files that match no source rule. That covers the recipe and its pipeline, and also a `.spitout` or `.spitdag` written into the dataset, which would otherwise be reported on the next scan.

**The root in a `.spitout`.** `spit inputs -o` starts the `.spitout` it writes with the root it settled against, so `dag` and `artifacts` on the `.spitout` need no `--root` either:

```text
root ../data
```

- It is relative to the `.spitout`'s folder, so the two can move together. A printed `.spitout` records no root: where it will be kept is unknown, so any root would be a guess, and one relative to the working folder is wrong after `> elsewhere/x.spitout`. It needs `--root`, as before.
- `--root` overrides it, as it overrides the recipe's line.
- It comes before every section, once. A `.spitout` without one, printed or written by hand, behaves as before: no root unless `--root` gives one.
- `inputs -o` records the root whenever it knows one: the flag, the `root` line, or the recipe's folder it scanned. A recipe whose written records are not scanned has a root only from its `root` line. So `dag` on a written `.spitout` checks that its source files exist, where before it did only with `--root`; planning from a `.spitout` without its data would need an opt-out, which waits until that workflow is needed.

**Checking.** `spit check` reads no data, but it can warn when the `root` folder does not exist, which the editor then shows on the line. A second `root` line is an error, as a second `pipeline` line is.

**Compatibility.** New syntax only. A recipe without the line behaves as today.

**Where.**

- **Parsing.** `header_lines` in `src/inputs/mod.rs` reads and blanks the `pipeline` and `root` lines; `InputSpec::root` keeps the folder and its line.
- **Resolution.** `settle` in `src/main.rs` returns the dataset root with what it settled; only the flag turns written records into a scan.
- **`.spitout`.** `SourceInventory::root` holds the line as written; `src/parser/inventory.rs` reads and writes it. `recorded_root` in `src/main.rs` makes it relative to the written file, and `prepare` joins it to the `.spitout`'s folder.
- **Scan.** `is_spit_file` in `src/inputs/discover.rs`.
- **Warning.** `missing_root` in `src/diagnostics.rs`.

**Tests.**

- A recipe with `root data` beside a `data/` folder: `inputs` and `dag` find the same sources and jobs as with `--root data`.
- `--root` overrides the line.
- `root ../data`, and an absolute root.
- A recipe with records and a `root` line is not scanned.
- A second `root` line is an error; a missing folder is a `check` warning.
- A recipe inside its own root is not reported as unmatched.
- A `.spitout` written to another folder records the root relative to itself, and `dag` on it verifies the files from any working folder; `--root` overrides it. Its `root` line is read once, before every section.

These are in `tests/dataset_root.rs`. The stored outputs lost their unmatched-file notes, which counted the fixture's own recipe, pipeline and `.spitout`.

**Guide.** The README's "Where files live" section, with a recipe-beside-the-data layout, which round 3 also asked for, and its `--root` row; the reference's Recipes and Inputs sections; `docs/architecture.md`. In spit-vscode, `root` is highlighted in a recipe and a `.spitout`.

## 2. Extensions on operation outputs

**Syntax.** An output type may be followed by an extension. A multi-output operation gives one per port:

```text
operation estimate_alignment(moving: Image<M,S>, reference: Image<N,T>) -> ToolTransform<S,T> .mat
operation fit(runs: many Data) -> (weights: Weights .npz, quality: Metrics .json) @ min(2)
```

An extension starts with `.` and may have several parts, such as `.nii.gz`.

**Optional throughout.** An operation need not declare an extension, and a pipeline need not use `ext:`. Without either, paths resolve exactly as today, including a default `path:` that ends in an extension. A declared extension adds a default and a check; it never forces a pipeline to restructure its path rules. Only the features that depend on knowing where the extension starts need it declared: `{x.stem}`, the sibling of a `beside` output, and the members of a `sidecars` group.

**Default.** `ext:` sets the extension for operations that declare none, at the top level or inside a stage, the same way `path:` does. Default path templates are then written without an extension:

```text
path: derivatives/{product}/{entities}
ext: .img
```

**Resolution.** A product's path is the first of:

1. its `path x:` rule, which must agree with the operation's extension, as below. When the operation declares none, the rule is used as written; `ext:` never applies to it;
2. the stage's or the top-level `path:` default, followed by the operation's extension, else the stage's `ext:`, else the top-level `ext:`, else nothing.

**Agreement.** When the operation declares an extension, a `path x:` rule for its output may:

- end with that extension, which is useful for full, BIDS-style paths;
- leave the extension off, and SPIT appends it;
- end with a different extension, which is an error.

The error matters: if the tool always writes `.mat`, a rule ending `.txt` names a file the tool never writes. The extension a template ends with is the text after its final placeholder, from the first `.`; dimension values cannot contain `.`, since they are escaped. A default template that ends with an extension while an operation declares another is the same error, with a hint to move the extension to `ext:`.

**Generic tools.** An operation whose tool picks the format from the output name declares nothing, and the path or default decides, as today. `import_photo`, `import_flat` and `export_tiff` all run `imgconvert`; only `export_tiff` commits to `.tif`. So the extension belongs on the operation, not the command.

**Sources** have no operation. Their own path rules keep their full extension. A source whose files a default rule finds takes `ext:`, like any product on a default rule, so a default stays one rule with one extension.

**Compatibility.** No current pipeline declares an extension or `ext:`, so every pipeline keeps its meaning.

**Built.** As above, with these details settled while building it:

- `-> .txt` gives an untyped output an extension, and `name: .ext` an untyped port one.
- A default rule that disagrees with several products' extensions is reported once, on the rule.
- An operation brought in with `use` keeps its extensions, and an imported source keeps the extension its own file's `ext:` gives it.
- A step whose output is named `ext`, as in `ext: Image = f(x)`, is still a step.
- `Pipeline::path_template_for` returns the completed template, so binding, discovery and the checks all see one path; `path_rule_for` gives the rule as written.
- `field_survey.spit` and `mrtrix3_act.spit` are converted, and `dag --paths` gives every artifact the same path as before. `mrtrix3_act.spit` uses a stage `ext:` for its parcellation stage, which stays NIfTI throughout.
- Tests are in `tests/extensions.rs`.

**Example.** In `field_survey.spit`, the 8 derived `path x:` lines go and every resolved path stays the same:

```text
# Generated images default to .img; operations that write another format declare it.
path: derivatives/{product}/{entities}
ext: .img

operation classify_map(image: Image<Map,S>) -> Image<Classes,S> .tif
operation export_tiff(image: Image<K,S>) -> Image<K,S> .tif
operation estimate_alignment(moving: Image<M,S>, reference: Image<N,T>) -> ToolTransform<S,T> .mat
operation convert_alignment(matrix: ToolTransform<S,T>, moving: Image<M,S>, reference: Image<N,T>) -> Transform<S,T> .txt
operation estimate_response(image: Image<Photo,S>) -> Response<Photo> .txt
operation trace_rows(vegetation: Image<Vegetation,S>, cover: Image<Cover,S>, seed: Image<Mask,S>) -> RowSet<S> .rows
operation weight_rows(rows: RowSet<S>, vegetation: Image<Vegetation,S>, cover: Image<Cover,S>) -> RowWeights .txt
operation build_yield_table(rows: RowSet<S>, regions: Image<Classes,S>, weights: RowWeights) -> YieldTable .csv
```

The other 15 image operations take `.img` from the default. The extension now sits beside the command that decides it: `.mat` beside `-omat {output}`. An operation shared through `use` carries its extension to every pipeline that imports it.

### Seeing the resolved path

With extensions inherited, a product's path is no longer written in one place, and agents said they wrote explicit paths partly to audit them. So:

- `spit check --path-rules` shows each product's completed path, and where an added extension is declared: ``yield_table (output): default derivatives/{product}/{entities}.csv, `.csv` from operation `build_yield_table` ``.
- For a pipeline that checks clean, `check --json` adds a `paths` list: each product whose path no rule writes in full, the line that declares it, and its path with `{product}` and `{stage}` written out. spit-vscode shows it as an inline hint at the end of that line, such as `→ derivatives/yield_table/{entities}.csv`, naming each product on a step with several outputs. A failed check clears the hints, since its lines may have moved.
- The agreement error is an ordinary diagnostic on the path rule's line:

```text
path `map_to_photo_matrix` ends in `.txt`, but operation `estimate_alignment` writes `.mat`; drop the extension or use `.mat`
```

## 3. Source groups

**Syntax.** A `sidecars` block declares sources that share dimensions and a path stem and differ only by extension. Its body is indented, like a [stage](../../docs/language-reference.md#stages):

```text
sidecars photo [site, visit, shot]: site-{site}/visit-{visit}/photos/site-{site}_visit-{visit}_shot-{shot}_photo
    source raw_photo : Image<Photo,Captured> .raw
    source photo_gps : GpsTrack .gpx
    source photo_imu : ImuTrace .imu
    source photo_json : CaptureMetadata .json

sidecars flat [site, visit]: site-{site}/visit-{visit}/calibration/site-{site}_visit-{visit}_flat
    source flat_field : Image<Flat,Captured> .raw
    source flat_field_json : CaptureMetadata .json
```

Each member is an ordinary source with the group's dimensions, and its path is the stem plus its extension. Calls such as `import_photo(raw_photo, photo_gps, photo_imu, photo_json)` do not change, and the type system is unaffected.

**Discovery.** Because SPIT knows the members belong together, `spit inputs` can report an incomplete group directly, for example `photo[site=A,visit=2,shot=3] has .raw and .gpx but no .imu`, instead of a missing match later during binding.

**Compatibility.** New syntax only.

**Built.** As above, with these details settled while building it:

- The report is a warning, not an error, until [optional members](#open-questions) are decided: a step that reads the missing file still fails at its join, and a step that does not is unaffected. A file an `exclude` rule removes is not counted as missing. Only a scan reports it; records written by hand are not checked.
- A member is written `source name : Type .ext`, or `source name .ext` untyped; it may not declare dimensions. A group may have no dimensions.
- Lowering turns each member into an ordinary source and `path` rule, so binding, discovery, imports and the path checks need nothing new; `Pipeline::sidecar_groups` keeps each group for the report. A `path` rule for a member is a duplicate, and two members with one extension bind to one path.
- A block belongs at the top level of a pipeline, not in a recipe or a stage.
- `field_survey.spit` declares its photo and flat-field sidecars as groups, and `mrtrix3_act.spit` its DWI runs and reverse b=0; every artifact keeps its path. Tests are in `tests/sidecars.rs`.

## 4. Implicit outputs

When a tool takes a path for each output, multiple outputs already work. When it writes one file beside another without being given a path for it, two things break: the command has nowhere to put the second placeholder, and the second output's default path is not where the tool writes it.

**Syntax.** `beside port` marks an output whose path is the named sibling's path without its extension, followed by the given text:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
operation skull_strip(t1: Image) -> (brain: Image .nii.gz, mask: Image "_mask.nii.gz" beside brain)
```

A `beside` output:

- may be left out of the command;
- has no path rule of its own, and a `path x:` rule for it is an error;
- names a sibling in the same operation, which has a declared extension and is not itself `beside` another.

The `.spitdag` is unchanged: a job's `outputs` already lists every output, whether or not its command mentions it.

**Built.** As above, with these details settled while building it:

- `{product}` in the sibling's path stays the sibling's name, since the file is written beside the sibling's: with `path: out/{product}/{entities}`, `meta` beside `image` is `out/image/sub=01.json`, not `out/meta/...`.
- A `beside` output's own extension is its suffix from the first `.`, so `"_mask.nii.gz"` gives `.nii.gz`, for [`{x.stem}`](#5-directory-and-stem-placeholders).
- `-> Json .json beside image` on an operation with one output is an error: `beside` names another output.
- `check --path-rules` reports it as `beside image`, and the editor shows its path like any other.
- Tests are in `tests/beside.rs`.

## 5. Directory and stem placeholders

Some tools take an output folder and a name rather than a path. `dcm2niix` takes `-o folder -f name` and adds `.nii.gz` and `.json` itself. Two placeholders cover this:

| Placeholder | Expands to |
| --- | --- |
| `{x.dir}` | The folder of output `x` |
| `{x.stem}` | The file name of `x` without its declared extension |

`{x.stem}` requires `x` to declare an extension, which is what makes the stem well defined for `.nii.gz`. Using either placeholder counts as using `x`. For a single unnamed output: `{output.dir}` and `{output.stem}`.

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -z y -b y -o {image.dir} -f {image.stem} {dicom}
```

**`.spitdag`.** A command part is literal text or `{"path": P}` today. These placeholders need parts that still name the artifact, such as `{"dir": P}` and `{"stem": P}`, so a backend knows which artifact the argument refers to. That is a format version bump.

## Not covered

- **BIDS derivative names.** Names such as `sub-{sub}_ses-{ses}_run-{run}_mc` vary with which dimensions a product has, so a shared template breaks on aggregates. An entity placeholder that writes only the dimensions present, in BIDS form, would address this better than a separate name or directory level.
- **Outputs a tool names unpredictably.** `dcm2niix` may add `_e2`, `_ph` or `_ROI1` depending on the data. SPIT cannot predict these. A declared output that does not appear should fail the job, which signals that the tool needs other flags or the data needs splitting.

## Open questions

- **Missing outputs.** Flags can contradict a declaration: with `-b n`, `dcm2niix` writes no `.json`. The [`.spitdag` format](../../docs/spitdag.md) does not say whether a backend must fail a job whose declared outputs are missing. It should, and this should be stated.
- **Optional members.** A BIDS `.json` may be absent. Should a group member, or a `beside` output, be allowed to be missing, perhaps using [optional types](../../docs/language-reference.md#optional-types)?
- **Literal dots.** The agreement rule reads a template's extension from the first `.` after its final placeholder. A literal name such as `report_v1.2` would be misread. Is that rare enough to accept, or should a path whose operation declares an extension be required to omit it?
- **Groups for derived products.** Is `beside` enough, or should an operation's outputs also be able to share a stem when the tool takes each path explicitly?

## Work order

1. The recipe's `root` line, the root recorded in the `.spitout`, the scan leaving out SPIT's own files, and the guide's layout example. Done.
2. Extensions on output ports, `ext:`, resolution and the agreement error. Return resolved templates from `spit check` and show them in the editor. Convert `field_survey.spit` and `mrtrix3_act.spit`. Done.
3. `sidecars` groups and the incomplete-group report in discovery. Done.
4. `beside` outputs. Done.
5. `{x.dir}` and `{x.stem}`, with the `.spitdag` version bump.
