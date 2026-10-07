# Recipe

A `.spitin` file binds a pipeline to one dataset. Its rules settle the source inventory before jobs are resolved. Recipe rules are not accepted in `.spit` files.

## Recipes

A `.spitin` recipe says how to find one dataset's inputs, keeping everything about the data out of the pipeline. A dataset that needs nothing but its folder needs no recipe: `spit dag analysis.spit --root data` scans the folder with the pipeline's own path rules. Its first line names the pipeline it serves, relative to the recipe's folder, and its `root` line the dataset folder:

```text
pipeline analysis.spit
root .

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
exclude image[sub=04,ses=2]    # scanner fault
exclude [sub] where sessions count<2
require [sub, ses] where image count=1
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

A recipe may contain `discover`, `exclude` and `require` rules, `path product:` rules for source products, a default `path:` rule for its sources, and `sources:`/`contexts:` records. It cannot declare sources, operations, steps, commands, stages, imports, or `ext:`; the pipeline still declares each logical `source` with its dimensions and optional type. Rules in a pipeline are an error, and so are records.

A recipe's `path:` line is the default for every source with no rule of its own, in the pipeline or the recipe. Where a dataset keeps its inputs is the dataset's to say, so the pipeline's `path:` can say where outputs go, by stage if it likes, and the recipe says where sources are:

```text
# analysis.spit
path: {@stage}/{@product}/{@entities}
source t1w : Image .nii.gz [sub]
source events .tsv [sub]

# dataset.spitin
pipeline analysis.spit
root .
path: rawdata/sub-{sub}/{@product}
```

A source takes the recipe's default only when it has no rule of its own, and the pipeline's default only when the recipe has none; see [Which file a line belongs in](#which-file-a-line-belongs-in). A pipeline default that names `{@stage}` outside a group finds no source, since no source is made in a stage, so it covers only outputs. Each source completes the recipe's default with the extension it declares, so one default finds `rawdata/sub-01/t1w.nii.gz` and `rawdata/sub-01/events.tsv`. `ext:` completes output paths and does not apply to the recipe's default, so a source that declares no extension takes the default as written. It cannot name `{@stage}`. `spit check recipe.spitin --path-rules` lists a source it covers as `default ... (recipe)`, and the `.spitout` writes it under `source_paths:` as each such source's rule.

A recipe names its dataset root, the folder its paths are relative to, once, on the line after `pipeline` by convention:

```text
pipeline analysis.spit
root data
```

The folder is relative to the recipe's folder, like the `pipeline` line, and may use `..` or be absolute; `root .` is the recipe's own folder. The line is required, so a recipe file always says where its data is: a recipe without one is an error, and no command-line option stands in for it. A pipeline has no `root` line. `spit check` warns when the folder is not there.

`spit check recipe.spitin` checks the rules against the pipeline without reading any data: each rule must name a source or discovery with the dimensions it counts, every source must have a path rule, by the pipeline, the recipe or a default, since the scan finds each source by its rule, and each source path the recipe gives, by its own rule or its default, must pass the [path checks](paths.md#paths), such as telling apart the sources a default covers. `spit inputs recipe.spitin` scans the root, applies the rules, and prints the `.spitout`. A recipe that writes its own `sources:` records is not scanned. Its `root` line only says where the dataset is: it does not make the recipe's records a scan, and their files must still exist under it. `spit dag recipe.spitin` runs the same step in memory before resolving jobs, over the pipeline the recipe's `pipeline` line names. A recipe is given alone; the pipeline is not named a second time on the command line.

Two forms of `exclude` leave data out; `require` checks what remains:

| To | Write | For example |
| --- | --- | --- |
| Remove named artifacts or groups, such as a corrupted run | `exclude` | `exclude bold[sub=02,ses=02,run=3]  # corrupted` |
| Remove every group that meets a condition, as the data changes | `exclude` | `exclude [sub] where sessions count<2` |
| Plan what can be completed despite missing inputs | `dag --partial` | `spit dag dataset.spitin --partial -o plan.spitdag` |
| Stop, when the data is incomplete | `require` | `require [sub, ses] where t1w count=1` |

Named exclusions apply first, even when written after conditional exclusions. Every conditional exclusion then sees the same retained inventory, and all matching groups are removed together. `require` checks what remains. Each removal is reported on stderr and recorded in the `.spitout`.

Conditional `exclude` and `require` name the groups first, then `where`, then the source or discovery rule and a condition. A conditional exclusion removes each group that meets its condition; `require` stops the run unless every group meets its own. A `require` in the older order, source first and the groups after `per`, as in `require t1w count=1 per [sub, ses]`, is an error that gives the rule rewritten. An old `drop` rule is rejected with an error that shows its `exclude` replacement.

Rules that count form their groups from every artifact and discovered context in the dataset, whichever source or discovery found it. `exclude [store] where pricing count=0` groups by every store any source or discovery has, so a store with sales but no price list is a group with none: its count is 0.

### Which file a line belongs in

A `.spit` pipeline is the reusable graph: what work to do and where its results go, for any dataset. A `.spitin` recipe binds that pipeline to one dataset: where its folder is, where its sources are when the pipeline does not say, and which of its data to leave out or require. So each line belongs in one file, except a path rule:

| Line | Pipeline | Recipe |
| --- | --- | --- |
| `source`, `dimensions`, `operation`, `command`, `verify`, steps, `stage`, `use`, `ext:` | yes | no |
| `path product:` for a product a step makes | yes | no |
| `path product:` for a source that is not declared `beside` another | either one, not both | either one, not both |
| `path:`, a default | covers outputs, and sources nothing else covers | covers sources only |
| `pipeline`, `root`, `discover`, `exclude`, `require`, `sources:`, `contexts:` | no | yes |

Put a source's own rule in the pipeline when every dataset for that pipeline shares the layout, and in the recipe when the layout belongs to one dataset. A line in the wrong file is an error that says which file it belongs in, at its line:

```text
error: line 3, column 1: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `discover`, `exclude` and `require` rules
error: line 3, column 13: `image` has path rules in both .spit and .spitin; keep the pipeline's if every dataset has this layout, or the recipe's if only this one does
```

A source takes the first path rule of these that it has:

1. its own `path source:` rule, from whichever file gives it;
2. the recipe's default `path:`;
3. the pipeline's default `path:`, unless it names `{@stage}` outside a group.

A rule written for one source comes before any default, and the dataset's default for its inputs comes before the pipeline's general one. An output's path never comes from the recipe. `spit check recipe.spitin --path-rules` shows which rule each source takes, and marks those the recipe gives `(recipe)`.

### Discover contexts from directories

```text
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
```

`discover` extracts global entity bindings from directories. `sessions` names the rule; it is not an artifact or an input to a step. Each matching directory contributes one `[sub=...,ses=...]` binding, including an empty directory. Values can be strings and need not be sequential. Only pairs found on disk are included; SPIT does not form a Cartesian product of subjects and sessions. The pattern is relative to the dataset root. `spit inputs` writes the bindings under `contexts sessions:`.

If a `discover` declaration matches no directories, discovery fails and names that declaration and its pattern.

Sources whose dimensions fit within the rule's dimensions expand over the observed bindings. For example, `source image [sub, ses]` expects one image per discovered pair, while `source reference [sub]` expects one per observed subject. Their `path` rules must name regular files; a missing file is an error. A source with another dimension, such as `run`, is still found by scanning its file path rule and can use `require` to check run coverage. The directory pattern must use every declared dimension, contain no other placeholders, and name a relative directory without `.` or `..` components. Values that cannot be represented faithfully in a `.spitout` are skipped with a warning.

`require` can target the name of a discovery rule directly:

```text
require [sub] where sessions count>=2
require [sub] where sessions has ses=1,2
```

The first rule needs at least two observed session bindings per subject. The second specifically needs sessions `1` and `2`. These rules count the directories matched by `sessions`, not artifacts from a product called `sessions`. Records keep the rule name as `contexts sessions:` followed by its `[sub=...,ses=...]` records, which `spit inputs` writes.

### Constraints

```text
require [subject, visit] where image count>=2
require [subject, visit] where reference count=1
```

Constraints, written in a recipe, check each observed group, and fail the run if any group fails. They do not set a total subject or visit count. The count takes any comparison: `count=1`, `count!=1`, `count>=2`, `count<=2`, `count>2` or `count<2`. A rule can also require particular values in each group with `has`, alone or after a count, as in `require [subject, visit] where image count>=2 has run=1,2`:

```text
require [subject, visit] where image has run=1,2
```

A `require` rule is checked after conditional exclusions, against the groups they leave. A rule whose grouping finds no group at all, because nothing in the dataset has those dimensions or an exclusion removed every one, is an error: a check of nothing is not a pass.

### Exclude groups that meet a condition

The [cohort walkthrough](https://github.com/eclnz/spit/tree/dev/examples) uses conditional `exclude` to remove a subject with too few sessions and named `exclude` to remove one damaged run.

Conditional `exclude` removes every group that meets its condition: the groups, then `where`, then what removes one.

```text
exclude [sub] where sessions count<2
exclude [sub, ses] where t1w count=0
exclude [sub, ses] where bold missing run=1,2
exclude [sub, ses] where bold has run=3
```

After `where` comes the source or discovery rule to count, then one condition:

- **A count,** with any comparison: `count<2` removes each group with fewer than two.
- **`missing` values:** `missing run=1,2` removes each group without a run 1 or without a run 2.
- **`has` values:** `has run=3` removes each group with a run 3.

Removing a group removes every artifact and discovered context within it, of every source. An artifact without all the group's dimensions, such as a subject's reference when only its sessions are removed, stays; if no job then uses it, `spit artifacts` lists it as unused. Each conditional `exclude` rule has one condition, and a group is removed when any rule's condition holds. Every rule is judged against the same inventory, so writing them in another order changes nothing. A conditional `exclude` rule that would remove every group of its grouping is an error, since nothing would be left to plan.

A file a discovered context expects but lacks counts as absent, so `exclude [sub, ses] where t1w count=0` removes a session whose T1w is missing, rather than failing on the missing file. Each removed group is reported on stderr, `note: excluded [sub=5] by \`exclude [sub] where sessions count<2\` (line 3); found 1`, and recorded in the `.spitout`.

A conditional `exclude` rule's values name a dimension within each group, not one of its groups: `exclude [store] where sales missing store=s07` is an error, since each group has one store. To remove named groups, write `exclude [store=s07]`.

### Exclude named artifacts

`exclude` removes artifacts by name, such as a corrupted run or a subject who withdrew, while their files stay where they are:

```text
exclude bold[sub=02,ses=02,run=3]    # corrupted: motion spike at volume 140
exclude [sub=07]                     # withdrew consent
exclude bold[run=3]                  # run 3 dropped from the protocol
exclude calibration                  # replaced by the pipeline's own
```

A rule names a source, some values, or both, and removes every artifact whose identity includes each value it names:

- **A source with all its dimensions** names one artifact.
- **Values alone**, in brackets, name a group: every source's artifacts with those values, and every discovered context that has them. `exclude [sub=02,ses=02]` removes a whole session.
- **A source with some of its dimensions** names part of that source only: `exclude bold[sub=02]` removes that subject's BOLD runs and nothing else, so steps other sources drive still run for them.

A comment on the line is kept as the rule's reason. Values are compared as written, so `sub=2` does not match `sub=02`. An exclude that matches nothing is an error, naming any value it comes close to, so a typo or a rule the data has outgrown does not pass unnoticed. `spit check` tests each rule against the pipeline: its source must be one, and each dimension it names must be that source's, or, for values alone, some source's.

Because values are compared as written, a group removed under one spelling keeps a source filed under another. Say store `s07`'s price list was filed as `pricing/S07.json`, so no job can price its sales, and the recipe removes the store for now:

```text
exclude [store=s07]            # price list filed as S07; renamed next week
```

Given a `.spitin` recipe, `inputs`, `dag` and `artifacts` list the spellings together as they settle its inputs, so the two are seen as one store filed twice. Only ASCII letters fold, so `é` and `É` are different values with no note; and a `.spitout` given directly to `dag` or `artifacts` has no settling step, so it gets no note. The note names each spelling with the sources that have it, and `(excluded)` for one a rule removed:

```text
note: excluded [store=s07] (line 3)
note: `store` has values that differ only in ASCII letter case, which are different values to SPIT: `S07` in pricing, `s07` (excluded)
```

The note comes up before any rule too, as ``... `S07` in pricing, `s07` in sales``. It names at most three sets for a dimension, then counts the rest. `dag` then plans the other stores, and notes that the misnamed file is left over:

```text
note: 1 source artifact is used by no job: pricing[store=S07]; `spit artifacts` lists them
```

The group rule did not remove it, since `S07` is not `s07`. A second rule names it, and the note goes:

```text
exclude [store=s07]            # price list filed as S07; renamed next week
exclude pricing[store=S07]     # the same list, under the name it was filed as
```

Named exclusions apply before scanning and missing-file validation. An excluded discovered context expects no files, an excluded file needs to exist nowhere, and a file excluded by name may lie outside every discovered context, such as a misnamed copy. Conditional exclusions then see what named exclusions leave, and `require` checks the final inventory.

A placeholder in a source rule matches any text within one folder or file name unless it takes a [shape](paths.md#shapes-on-a-source-placeholder), as `{date:date}`. Without one, `logs/{server}/{date}.log` reads `logs/web1/notes.log` as `date=notes`: check the count in `note: found N source artifacts`, and leave out a file whose value does not belong with [`exclude`](#exclude-named-artifacts), a shape, or a rule that names more of its path. Files whose whole paths match no source path rule are ignored while scanning. `spit inputs` counts them in a note, naming them when there are at most three and otherwise counting them by extension, leaving out SPIT's own `.spit`, `.spitin`, `.spitout` and `.spitdag` files and any file at a path the pipeline gives one of its outputs, or inside an output folder, such as what an earlier run wrote under the root; `spit inputs dataset.spitin --unmatched` lists their paths relative to the dataset root instead of writing a `.spitout`, even if a `require` rule fails. When a `require` count fails after the scan found no files for its source, `inputs` and `dag` name the path rule used and show an unmatched file whose path contains the source name, when there is one. A recipe's `path:` is a default for sources; use `path <source>:` for one source. Required source paths must match the spelling found by the scan: `pricing/S07.json` does not satisfy `pricing/s07.json`, even on a case-insensitive filesystem. A file with a near miss in an identity value, such as `store=S07` where a job needs `store=s07`, may still match a source rule: it is then a source artifact, and `dag` and `artifacts` warn when it is unused and point to it at the failed join.

Rules can also come from a CSV file, relative to the recipe's folder, such as a lab's list of scans that failed quality control:

```text
exclude from qc/excluded.csv
```

```csv
product,sub,ses,run,reason
bold,02,02,3,"motion spike, volume 140"
,07,,,withdrew consent
```

The header names the columns: `product` and `reason` are optional, and every other column is a dimension. Each row is one rule; an empty cell leaves its column out, so a row with no `product` names a group. Fields may be quoted, as spreadsheets write them. Each row must match something, and a message about a row names the file and its line.
