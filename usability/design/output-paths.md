# Design: output extensions and sidecars

Status: proposal. Nothing here is implemented.

## Problem

Agents in every usability round called path rules repetitive; in [round 3](../ROUND3.md) it was, with choosing `--root`, the biggest first-build friction in the cohort task. Most of that repetition takes one of two forms.

**Overrides that only change the extension.** In [`field_survey.spit`](../../examples/commands/field_survey/field_survey.spit), all 8 derived `path x:` rules copy the default `derivatives/{product}/{entities}` to change `.img` to `.tif`, `.mat`, `.txt`, `.rows` or `.csv`. In [`mrtrix3_act.spit`](../../examples/commands/mrtrix3_act/mrtrix3_act.spit), all 8 derived rules, stage defaults included, do the same. Each copy also stops inheriting from the default: change `derivatives/` to `out/` and the 8 overrides silently keep the old directory.

**Sidecars.** `photo_gps`, `photo_imu` and `photo_json` are `raw_photo`'s path with another extension, and each repeats the full template and dimension list. On the derived side, a tool such as `dcm2niix` writes a `.json` beside its image without being given a path for it, which SPIT cannot express today: a command must use every output placeholder, and each output gets its own path rule.

A third form, BIDS derivative names whose entity labels change with each product's dimensions (`…_run-{run}_mc` beside `…_avg`), is out of scope here. See [Not covered](#not-covered).

## Overview

The file format is chosen by the tool, so the extension belongs on the operation that runs it, not on each product's path. Four changes build on that:

1. [Extensions on operation outputs](#1-extensions-on-operation-outputs), with a default for operations that declare none.
2. [Source groups](#2-source-groups) for sidecars in the input data.
3. [Implicit outputs](#3-implicit-outputs): `beside` for files a tool writes next to another output.
4. [Directory and stem placeholders](#4-directory-and-stem-placeholders) for tools that take a folder and a name instead of a path.

Each later change depends on SPIT knowing an output's extension, so they land in this order. Alongside 1, `spit check` reports each product's resolved path, so the editor can show it (see [Seeing the resolved path](#seeing-the-resolved-path)).

## 1. Extensions on operation outputs

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

**Sources** have no operation. Their path rules keep their full extension.

**Compatibility.** No current pipeline declares an extension or `ext:`, so every pipeline keeps its meaning.

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

With extensions inherited, a product's path is no longer written in one place, and agents said they wrote explicit paths partly to audit them. `spit check` already resolves every product's template to validate it. It should also return each resolved template, and where each part came from, so the VS Code extension can show `derivatives/yield_table/{entities}.csv` beside `yield_table = …`. The agreement error appears as an ordinary diagnostic, pointing at both the path rule and the operation:

```text
path map_to_photo_matrix ends in .txt, but estimate_alignment writes .mat; drop the extension or use .mat
```

## 2. Source groups

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

## 3. Implicit outputs

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

## 4. Directory and stem placeholders

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

1. Extensions on output ports, `ext:`, resolution and the agreement error. Return resolved templates from `spit check` and show them in the editor. Convert `field_survey.spit` and `mrtrix3_act.spit`.
2. `sidecars` groups and the incomplete-group report in discovery.
3. `beside` outputs.
4. `{x.dir}` and `{x.stem}`, with the `.spitdag` version bump.
