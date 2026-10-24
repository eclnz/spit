# SPIT documentation

SPIT turns a description of artifact families and operations into a concrete job plan. A pipeline describes *what* to make; a recipe or input inventory describes *what this dataset contains*. SPIT checks how they fit together and writes a `.spitdag` for a runner. It does not execute the jobs itself.

## Choose a starting point

| If you want to… | Read |
| --- | --- |
| Plan your first pipeline | [Getting started](getting-started.md) |
| Understand the files and their flow | [Files and flow](guide/concepts.md) |
| Write operations, commands, checks, and stages | [Pipelines and operations](guide/pipelines.md) |
| Add type checks or share definitions across files | [Types and reusable definitions](guide/types-and-reuse.md) |
| Join, broadcast, or collect artifacts | [Matching and collections](guide/matching.md) |
| Find source files and name output files | [Paths and file kinds](guide/paths.md) |
| Describe a dataset and its exceptions | [Recipes and input inventories](guide/recipes.md) |
| Find out why a job is missing | [Inspecting and diagnosing plans](guide/inspection.md) |
| Look up exact syntax | [Language manual](language-reference.md) |
| Look up flags or DAG fields | [Command line](cli.md) · [DAG format](spitdag.md) |

The [examples](examples.md) show complete pipelines and datasets, including a ragged parameter sweep, a cohort with missing inputs, and a sensor pipeline with selectors and two outputs.

## Reading order

Start with [files and flow](guide/concepts.md) to see how the pipeline, recipe, plan, and runnable DAG fit together. Then follow each guide page's **Next** link. Use the [manual](language-reference.md) when you need the exact valid form or behavior of a language component.
