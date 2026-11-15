# SPIT documentation

SPIT turns a description of artifact families and operations into a concrete job plan. A pipeline describes *what* to make; a recipe or input inventory describes *what this dataset contains*. SPIT checks how they fit together and writes a `.spitdag` for a runner. It does not execute the jobs itself: the `spit-bash` runner does.

## The user guide

The guide follows one pipeline from its first run to its upkeep. Read it in order, or jump to the page that answers your question.

| If you want to… | Read |
| --- | --- |
| Install SPIT and run a pipeline | [First run](guide/first-run.md) |
| Read one real analysis from data to figure | [A weather network](guide/walkthrough.md) |
| Name, type and locate files, folders and their companions | [Artifacts](guide/artifacts.md) |
| Declare commands, tools, resources and checks | [Operations](guide/operations.md) |
| Connect steps; join, collect or broadcast artifacts; reuse definitions | [Pipelines](guide/pipelines.md) |
| Describe a dataset, leave data out, require data or wait for a review | [Datasets](guide/datasets.md) |
| Inspect a plan and find out why a job is missing | [Plans](guide/plans.md) |
| Run a plan, rerun only what changed, and manage its files | [Runs](guide/runs.md) |
| Find a concrete example of any feature | [The weather family](guide/weather.md) |
| Fix an error or find out why a job reran | [Troubleshooting](guide/troubleshooting.md) |
| See types, paths and path hints as you write | [The editor](guide/editor.md) |
| Look up a term, a clause or a command | [Glossary and cheat sheet](guide/glossary.md) |

## The manuals

Look up a command, the plan's format, or a complete example.

| If you want to… | Read |
| --- | --- |
| Look up a command's flags and output | [Command line](cli.md) |
| Write or read a plan | [DAG format](spitdag.md) |
| See complete pipelines and datasets | [Examples](examples.md) |

The [examples](examples.md) include a ragged parameter sweep, a cohort with missing inputs, and a sensor pipeline with selectors and two outputs.
