# How SPIT thinks about data

SPIT describes *families* of artifacts. A `source` is an existing family; a step makes a new one. An artifact is one member of a family, identified by values for its dimensions.

```spit
source image : Image [subject, visit, run]
source atlas : Image
```

`image[subject=A,visit=1,run=2]` is one artifact. `atlas` has no dimensions and is one artifact shared by jobs that read it. Types such as `Image` are optional; the identity is the product name and its dimension values. [Product syntax](../language-reference.md#products-and-dimensions) gives the full rules.

## A pipeline defines families; inputs supply members

```spit
operation clean(input: Image) -> Image
command clean: clean_tool {input} {@output}
cleaned = clean(image)
```

The `.spit` file defines the source family, operation, and derived `cleaned` family. It does not say which subjects exist. A `.spitout` can supply observed members:

```text
sources:
    image[subject=A,visit=1,run=1]
    image[subject=A,visit=1,run=2]
    atlas
```

This produces two `cleaned` jobs. SPIT starts with observed artifacts; it does not automatically form every combination of values it has seen. `spit inputs` can create the `.spitout` by scanning a folder using [path rules](paths.md), or a recipe can supply records and discovery rules.

## A step binds an operation to products

`cleaned = clean(image)` names the product the operation makes. The operation describes its ports and command; the step names actual input and output families. A later step can read `cleaned`. SPIT resolves dependencies and writes jobs in runnable order. An operation may be called in several steps and may have several named outputs.

Jobs are determined by matching artifact identities at the step. An input with the most dimensions usually drives the step. Other inputs must match it as exactly one artifact, unless a [selector](matching.md) changes that matching. A missing or ambiguous match is reported before a job is emitted.

## Dimension order matters

Every product's dimensions follow one pipeline-wide order. Each source orders the dimensions in its own declaration: `source seed [config, seed]` puts `config` before `seed`. If a step combines dimensions whose relative order no source establishes, add one top-level `dimensions` line naming every dimension. For example, broadcasting `model` over seeds grouped by `config` combines dimensions from separate sources:

```spit
dimensions [model, config, seed]
source model [model]
source seed [config, seed]
operation train(model, seed) -> Weights
trained = train(model @ each(model), seed)
```

Neither source says whether `model` comes before `config`; the `dimensions` line settles that. Conflicting orders in source declarations are an error. The pipeline-wide order controls displayed identities, `{@entities}` in paths, and the order of artifacts passed through a `many` command placeholder. SPIT checks declared output dimensions against the inferred ones. See [dimension order](../language-reference.md#dimension-order).

## Files have distinct roles

| File | Contains | Reads from |
| --- | --- | --- |
| `.spit` | Reusable graph and path rules | Other `.spit` files through `use` |
| `.spitin` | Dataset root, source paths, discovery and removal rules | Its `.spit` pipeline and the dataset |
| `.spitout` | Settled source identities, paths and removals | No pipeline; pass one to `dag` |
| `.spitdag` | Concrete artifacts, jobs, commands, dependencies | A runner, without the earlier files |

The [recipes guide](recipes.md) explains when each input form is useful. The [architecture page](../architecture.md) describes the compiler phases if you are changing SPIT itself.
