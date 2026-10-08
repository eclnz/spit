# Files and flow

SPIT separates a reusable description of work from the dataset used for one run. It checks both, plans concrete jobs, and writes a DAG for a runner. SPIT itself does not execute those jobs.

| Stage | File or command | Responsibility |
| --- | --- | --- |
| Pipeline | `.spit` | Declare source and output families, operations, steps, and reusable definitions. It contains no particular dataset's source records. |
| Recipe | `.spitin` | Select a pipeline and dataset root; describe how to find, exclude, and require source data for this dataset. |
| Plan | `.spitout` and `spit dag` | Settle observed source records in an optional inventory, then inspect the jobs and missing inputs that result from combining it with the pipeline. A recipe can be passed directly to `dag` without saving an inventory. |
| Runnable DAG | `.spitdag` | Save concrete artifacts, paths, jobs, commands, and dependencies. A runner reads this file without needing the earlier files. |

```text
pipeline.spit + dataset.spitin + data/
    └─ spit inputs ─► inputs.spitout (optional saved inventory)

pipeline.spit + inputs.spitout
    └─ spit dag ─► inspect the plan ─► plan.spitdag ─► runner
```

The inventory records what exists; the plan decides what can be made. `spit check` validates declarations, `spit inputs` settles a recipe, and `spit dag` previews or saves concrete jobs. When source paths are already in the pipeline, `spit dag pipeline.spit --root data` can scan them without a recipe. For exact file syntax, see the [language manual](../language-reference.md) and [DAG format](../spitdag.md).

Start with the pipeline when deciding what work is reusable. Add a recipe when dataset layout or coverage rules vary. Inspect the plan before handing a DAG to a runner.

Next: [Pipelines](pipelines.md).
