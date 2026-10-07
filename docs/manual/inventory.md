# Input inventory

A `.spitout` is a textual inventory of settled source artifacts. It can be generated or authored directly. It does not name its pipeline; consumers receive the pipeline separately.

## Inputs

A `.spitout` lists a dataset's settled source identities. `spit inputs` writes one, and a dataset indexer or a person can write one too. Paths come from rules in the pipeline or, if a recipe supplies a source rule, a `source_paths:` section written once in the `.spitout`. A source with records needs one or the other; `dag` fails one with neither rather than guess where its files are. `contexts:` names a group even when one of its required inputs is absent:

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]
```

For one named directory discovery, `spit inputs` nests source identities under each context. A list of values in a nested dimension expands each listed product for every value:

```text
sources:
    source_lut
contexts sessions:
    [sub=01,ses=01]:
        reverse_b0, t1w
        [run=01,02]:
            raw_dwi, dwi_bvec, dwi_bval, dwi_json
```

This declares two runs of each listed DWI source. Flat `product[dimension=value,...]` records remain valid. A source path rule declared only in a recipe is written once in the `.spitout`:

```text
source_paths:
    image: data/sub-{sub}/image.nii.gz
```

The DAG can then use the rule without loading the recipe. A record names no file of its own: its source's path rule gives it.

A `.spitout` that `spit inputs -o` writes starts with the dataset root it was settled against:

```text
root ../data
```

The folder is relative to the `.spitout`'s own folder, and may be absolute. A printed `.spitout` records no root, since where it will be kept is unknown. `dag` and `artifacts` use it as the root, so they check the source files and run commands from it. The line comes before every section, once. A `.spitout` without one, printed or written by hand, has no root, so `dag` checks no source files; give it a `root` line, or run `dag` on the recipe.

`spit inputs` also writes what the recipe's `exclude` rules removed, each with its rule, where the rule is, how many a counting rule found, and the reason:

```text
removed:
    bold[sub=02,ses=02,run=3]
        rule: exclude bold[sub=02,ses=02,run=3]
        at: line 4
        reason: corrupted: motion spike at volume 140
    [sub=07]
        rule: exclude [sub] where sessions count<2
        at: line 6
        found: 1
```

The section is a record, not a rule: the records above it already leave these out, and resolving jobs removes nothing more. Scanning again rewrites it from the recipe, so a removal survives a rescan. `dag` copies it into the `.spitdag`.

`dag --partial` is a choice for one planning run, not a recipe rule. It keeps jobs with complete inputs, including aggregate jobs whose `many` input still has complete members, and writes the other outputs with their reasons in the `.spitdag`'s `left_out` array. A `many` input's `@ min(count)` is checked after incomplete members have been removed. Plain `dag` still fails on the first incomplete job and points to `artifacts` and `dag --partial`.
