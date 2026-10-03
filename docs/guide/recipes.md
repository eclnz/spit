# Recipes and input inventories

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

A recipe's `path image:` names a source; its `path:` is a default for sources. A pipeline's `path:` is primarily for outputs and covers sources only when they have no more specific rule and the default fits them. A source's own rule belongs in either the pipeline or the recipe, not both. A recipe can also give the stem of a `sidecars` group without a pipeline stem. See [which file a line belongs in](../language-reference.md#which-file-a-line-belongs-in).

## Discover observed contexts

```spit
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
```

Each matching directory contributes an observed `[sub=...,ses=...]` context, including an empty directory. Sources within those dimensions can then be expected for each context. SPIT does not invent subject/session combinations. The [discovery reference](../language-reference.md#discover-contexts-from-directories) explains how source paths and missing files interact with contexts.

## Leave out data or enforce coverage

```spit
exclude bold[sub=02,ses=01,run=3]  # corrupted scan
drop [sub] where sessions count<2
require t1w count=1 per [sub, ses]
```

`exclude` names artifacts or groups to remove while their files remain on disk. `drop` removes each group meeting a condition, such as too few sessions. `require` fails when a retained group lacks the required count or values. They apply in that order, regardless of line order. `exclude from qc/excluded.csv` can read a list of exclusions. The [recipe reference](../language-reference.md#recipes) and its [constraints](../language-reference.md#constraints), [drop](../language-reference.md#drop-groups-that-fail-a-criterion), and [exclude](../language-reference.md#exclude-named-artifacts) sections give the full syntax and failure rules.

## Use or write a `.spitout`

`spit inputs dataset.spitin -o dataset.spitout` writes the settled source identities and source paths. A hand-written `.spitout` works too:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
```

`spit dag pipeline.spit dataset.spitout` uses those records. A `.spitout` may also hold a `root`, `source_paths:`, `contexts`, and `removed:` records. A printed or hand-written inventory without `root` does not make `dag` check that source files exist. The [inputs reference](../language-reference.md#inputs) describes the complete format.

Next: [Inspecting and diagnosing plans](inspection.md).
