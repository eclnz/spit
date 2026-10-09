# Recipes

A `.spitin` recipe binds a reusable pipeline to a dataset. It names the pipeline and the root folder containing source data:

```spit
pipeline analysis.spit
root data
```

Both paths are relative to the recipe's own folder. If the pipeline already declares a root, leave the recipe's out to inherit it; declaring a root in both is an error. Run `spit inputs dataset.spitin` to see the settled inputs, or `spit dag dataset.spitin` to resolve jobs directly. When the pipeline already gives every source a path and no dataset rules are needed, put `root data` in the pipeline and run `spit dag analysis.spit` without a recipe. A pipeline without a root can instead take `--root data`, relative to the working folder. All selection rules, including simple literal exclusions, require a recipe.

## Source paths

```spit
path image: raw/sub-{sub}/image.nii.gz
path: raw/{@product}/{@entities}
```

A recipe's `path image:` names a source; its `path:` is a default for sources. A pipeline's `path:` is primarily for outputs and covers sources only when they have no more specific rule and the default fits them. A source's own rule belongs in either the pipeline or the recipe, not both. A recipe can also give the complete path of a main source whose companions are declared `beside` it. See [which file a line belongs in](../language-reference.md#which-file-a-line-belongs-in).

## Discovery

```spit
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
```

Each matching directory contributes an observed `[sub=...,ses=...]` context, including an empty directory. Sources within those dimensions can then be expected for each context. SPIT does not invent subject/session combinations. The [discovery reference](../language-reference.md#discover-contexts-from-directories) explains how source paths and missing files interact with contexts.

## Coverage

```spit
exclude bold[sub=02,ses=01,run=3]  # corrupted scan
exclude [sub] where sessions count<2
require [sub, ses] where t1w count=1
```

`exclude` removes named artifacts or groups, or every group meeting a condition, while their files remain on disk. `require` fails when a retained group lacks the required count or values. Named exclusions apply first, then conditional exclusions, then requirements, regardless of line order. `exclude from qc/excluded.csv` can read a list of exclusions. The [recipe reference](../language-reference.md#recipes), [conditional exclusion](../language-reference.md#exclude-groups-that-meet-a-condition), and [constraints](../language-reference.md#constraints) sections give the full syntax and failure rules.

## Inventories

`spit inputs dataset.spitin -o dataset.spitout` writes the settled source identities and source paths. A hand-written `.spitout` works too:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
```

`spit dag pipeline.spit dataset.spitout` uses those records. A `.spitout` may also hold a `root`, `source_paths:`, `contexts`, and `removed:` records. A printed or hand-written inventory without `root` does not make `dag` check that source files exist. The [inputs reference](../language-reference.md#inputs) describes the complete format.

Next: [Plans](inspection.md).
