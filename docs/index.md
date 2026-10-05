# SPIT documentation

SPIT turns a description of artifact families and operations into a concrete job plan. A pipeline describes *what* to make; a recipe or input inventory describes *what this dataset contains*. SPIT checks how they fit together and writes a `.spitdag` for a runner. It does not execute the jobs itself.

## Choose a starting point

| If you want to… | Read |
| --- | --- |
| Plan your first pipeline | [Getting started](getting-started.md) |
| Understand products, dimensions, and jobs | [How SPIT thinks about data](guide/concepts.md) |
| Write operations, commands, checks, and stages | [Pipelines and operations](guide/pipelines.md) |
| Add type checks or share definitions across files | [Types and reusable definitions](guide/types-and-reuse.md) |
| Join, broadcast, or collect artifacts | [Matching and collections](guide/matching.md) |
| Find source files and name output files | [Paths and file kinds](guide/paths.md) |
| Describe a dataset and its exceptions | [Recipes and input inventories](guide/recipes.md) |
| Find out why a job is missing | [Inspecting and diagnosing plans](guide/inspection.md) |
| Look up exact syntax | [Complete language reference](language-reference.md) |
| Browse every keyword, selector, and placeholder | [Language catalog](reference/index.md) |
| Look up flags or DAG fields | [Command line](cli.md) · [DAG format](spitdag.md) |

The [examples](examples.md) show complete pipelines and datasets, including a ragged parameter sweep, a cohort with missing inputs, and a sensor pipeline with selectors and two outputs.

## The four files

| File | Purpose | Usually written by |
| --- | --- | --- |
| `.spit` | Reusable sources, operations, steps, commands, and output paths | You |
| `.spitin` | Dataset root, source paths, discovery and filtering rules | You, when a dataset needs one |
| `.spitout` | Settled source artifacts and their identities | `spit inputs`, or an indexer |
| `.spitdag` | Bound jobs, paths, commands and dependencies | `spit dag` |

You can give `dag` a recipe directly, or give it a pipeline and a `.spitout`. When the pipeline's own path rules find all sources, a pipeline with `--root` is enough. The [getting started guide](getting-started.md) shows both the file flow and the commands.
