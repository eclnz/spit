# Paths

A path rule turns an artifact identity into a path relative to the dataset root. A pipeline can set a default, and a particular product can override it:

```spit
path: results/{@product}/{@entities}
ext: .nii.gz
source image : Image .nii.gz [sub, run]
path image: raw/sub-{sub}/run-{run}.nii.gz
```

Here the `image` source has its own path; derived products take the default. An output with no rule uses the built-in `out/{@product}/{@entities}`. A source has no built-in path, so it needs a rule from the pipeline, recipe or `.spitout`. `spit check --path-rules` shows the effective rule and where it came from.

## Path placeholders

| Placeholder | Value |
| --- | --- |
| `{sub}`, `{run}`, etc. | A dimension value |
| `{@product}` | The product name |
| `{@entities}` | All dimension assignments, or `global` |
| `{@labels}` | BIDS-style `key-value` labels |
| `{@stage}` | The stage path of a derived product |

SPIT's placeholders start with `@`; a plain name is a dimension. `[text]` makes a path segment optional when its placeholders have no value for that product. For example, `sub-{sub}[/ses-{ses}][/{@stage}]/{@labels}_{@product}` can cover both session and subject outputs. Every resolved product still gets one concrete template. The [path reference](../manual/paths.md#paths) covers literal braces, validation, collision checks, and how path values are escaped.

## Extensions and output names

An operation can declare the extension its output file has; `ext:` supplies a default when the operation does not. Sources can declare their extensions too. A default rule can therefore omit the suffix:

```spit
path: derived/{@product}/{@entities}
ext: .csv
source events .tsv [sub]
operation analyse(events) -> Report .json
```

The source takes `.tsv` and the operation's output takes `.json`; `ext: .csv` applies only where no output extension is declared. See [extensions](../manual/paths.md#extensions) for exact precedence and errors.

For a tool that writes a second output next to the first without taking its path, declare it `beside` the first output. The companion follows the first output's stem with a different suffix:

```spit
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -o {image.dir} -f {image.stem} {dicom}
```

`{image.dir}` and `{image.stem}` give a tool the destination folder and the name without its extension. Both count as using the output. A `beside` output has no independent path rule. See [files a tool writes beside another](../manual/paths.md#files-a-tool-writes-beside-another).

## Folder artifacts and sidecar sources

Add `/` after a product's type or extension for an artifact that is a folder:

```spit
source dicom : Dicom / [sub]
operation recon(dicom: Dicom) -> FsSubject /
```

The path rule names the folder without a trailing slash. SPIT checks folder sources as folders and prevents a job's output folder from overlapping other products' paths. See [folders](../manual/paths.md#folders).

Declare a main source, then declare each companion `beside` it when source files share a stem:

```spit
source raw : Image .raw [site, shot]
source meta : Json .json beside raw
path raw: photos/{site}/{shot}.raw
```

The companion inherits the main source's dimensions and path stem; a recipe can instead provide `path raw: ...` for the main source. If a companion is missing for an identity, SPIT warns. An output declared `beside` another output is written by the same job. See [sidecar files](../manual/paths.md#sidecar-files).

A source path placeholder can restrict the values it reads: `{run:digits}`, `{year:year}`, and `{date:date}` accept digits, four-digit years from 1900 to 2099, and real `YYYY-MM-DD` dates respectively. Shapes belong only on paths that read sources or discover directories. See [shapes on a source placeholder](../manual/paths.md#shapes-on-a-source-placeholder).

Next: [Types](types-and-reuse.md).
