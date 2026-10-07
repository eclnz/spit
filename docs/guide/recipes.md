# Recipe

A `.spitin` recipe binds a reusable pipeline to a dataset. It names the pipeline and the root folder containing source data:

```spit
pipeline analysis.spit
root data
```

Both paths are relative to the recipe's own folder. Run `spit inputs dataset.spitin` to see the settled inputs, or `spit dag dataset.spitin` to resolve jobs directly. When the pipeline already gives every source a path and no dataset rules are needed, `spit dag analysis.spit --root data` works without a recipe.

## Give sources dataset-specific paths

```spit
path image: raw/sub-{sub}/image.nii.gz
path: raw/{@product}/{@entities}
```

A recipe's `path image:` names a source; its `path:` is a default for sources. A pipeline's `path:` is primarily for outputs and covers sources only when they have no more specific rule and the default fits them. A source's own rule belongs in either the pipeline or the recipe, not both. A recipe can also give the complete path of a main source whose companions are declared `beside` it. See [which file a line belongs in](../manual/recipe.md#which-file-a-line-belongs-in).

## Discover observed contexts

```spit
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
```

Each matching directory contributes an observed `[sub=...,ses=...]` context, including an empty directory. Sources within those dimensions can then be expected for each context. SPIT does not invent subject/session combinations. The [discovery reference](../manual/recipe.md#discover-contexts-from-directories) explains how source paths and missing files interact with contexts.

## Leave out data or enforce coverage

```spit
exclude bold[sub=02,ses=01,run=3]  # corrupted scan
exclude [sub] where sessions count<2
require [sub, ses] where t1w count=1
```

`exclude` removes named artifacts or groups, or every group meeting a condition, while their files remain on disk. `require` fails when a retained group lacks the required count or values. Named exclusions apply first, then conditional exclusions, then requirements, regardless of line order. `exclude from qc/excluded.csv` can read a list of exclusions. The [recipe reference](../manual/recipe.md#recipes), [conditional exclusion](../manual/recipe.md#exclude-groups-that-meet-a-condition), and [constraints](../manual/recipe.md#constraints) sections give the full syntax and failure rules.

Next: [Input inventory](inventory.md).
