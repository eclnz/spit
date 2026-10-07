# Runnable DAG

After checking the pipeline and its inputs, inspect the work before handing it to a runner:

```sh
spit dag dataset.spitin --counts --commands
spit dag dataset.spitin --paths
spit dag dataset.spitin -o plan.spitdag
```

The saved file contains concrete jobs, their inputs and outputs, command arguments, and dependencies. You no longer need the earlier files to execute this plan. You do need the dataset and the programs named by the commands. [spit-bash](https://github.com/eclnz/spit-bash) is a local runner.

## Check the handoff

Look for an unexpected job count or path before running tools. Confirm the recorded root is the intended dataset and the commands use the expected inputs. If the plan was made from a rootless inventory, the runner needs to know where those relative paths belong.

The runner performs input checks and verification before a command, requires every declared output after it, and performs output checks. A failed job blocks its consumers. Declare only outputs the tool will actually create. The [DAG specification](../manual/dag.md) defines these obligations for runner authors.

## Replan when inputs change

A DAG is the runnable plan for one inventory. Adding input files does not add jobs to an existing DAG. Settle inputs again and generate a new plan. Runner policies decide when existing outputs need rerunning; SPIT’s job fingerprints identify work, not the contents of input files.

Next: [Inspection](inspection.md).
