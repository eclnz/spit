# File flow

SPIT helps you describe work once and plan it for different datasets. Start by separating four questions: **What work should happen? Where is this dataset? What inputs are available? Which jobs can run?** Each question has a file.

| File | Question | Responsibility |
| --- | --- | --- |
| **Pipeline** `.spit` | What work should happen? | Declare source families, operations, steps, and output naming. Reuse it across datasets. |
| **Recipe** `.spitin` | Where is this dataset, and which data should be used? | Name a pipeline and root; describe source layout, discovery, exclusions, and requirements. |
| **Input inventory** `.spitout` | What inputs are available after those rules? | Record settled source identities, supplied source paths, and removals. |
| **Runnable DAG** `.spitdag` | Which concrete jobs can run? | Record bound paths, command arguments, checks, and dependencies for a runner. |

## Direction of flow

```text
Pipeline (.spit) ───────────────┐
       │                       │
       ▼                       ▼
Recipe (.spitin) + dataset → Input inventory (.spitout)
                               │
                    Pipeline + inventory
                               │
                               ▼
                       Runnable DAG (.spitdag)
                               │
                               ▼
                             Runner
```

`spit check` validates the declarations. `spit inputs` reads the dataset and settles its inventory. `spit dag` combines the pipeline with that inventory and writes the runnable DAG. A runner, such as [spit-bash](https://github.com/eclnz/spit-bash), executes its jobs. SPIT itself plans work; it does not run the tools.

You can pass a recipe directly to `dag`; it builds the inventory in memory. You can also save an inventory for inspection or supply one from an indexer. A DAG belongs to the specific inventory it was planned from: after changing the inputs, make a new DAG.

## Separation of concerns

Keep reusable work in the pipeline. Keep a dataset’s root, exceptions, and coverage policy in its recipe. The inventory records the result of applying that policy; the runnable DAG records the result of matching that inventory to the work. A runner needs only the DAG and access to the data and executables. Queue settings, process logs, and rerun policy belong to the runner.

A small pipeline does not always need all four files on disk. If its source paths already describe the dataset and it needs no recipe rules, `spit dag analysis.spit --root data` scans and plans directly. If it needs exclusions or requirements, put them in a recipe. Exact file ownership and root rules are in the [manual](../manual/recipe.md#which-file-a-line-belongs-in).

## Reading this guide

First build a working pipeline, then learn how product identities control the jobs it makes. The **Pipeline** chapters cover operations, matching, paths, types, and reuse. **Recipe** explains dataset policy. **Input inventory** explains settled inputs. **Runnable DAG** explains inspection and handing work to a runner. Complete examples follow.

For accepted syntax and boundary cases, use the [user manual](../manual/index.md). Each topic has one authoritative specification there; this guide explains when and how to use it.

Next: [First pipeline](first-pipeline.md).
