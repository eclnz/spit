# Architecture boundary

SPIT compiles a reusable pipeline, settles dataset inputs, resolves jobs, and binds them into a runnable DAG. The [guide](guide/concepts.md) explains the user-facing file flow. These pages describe the contributor-facing data model and module boundaries.

## Three steps

1. **Compile:** `.spit` becomes a checked `Pipeline`; see [compiler stages](architecture/phases.md).
2. **Build inputs:** a recipe and dataset become a `SourceInventory`, serialized as `.spitout`; see [compiler stages](architecture/phases.md).
3. **Resolve and bind:** the pipeline and inventory become a `BoundDag`, serialized as `.spitdag`; see [binding and output](architecture/binding.md).

A runner reads only `.spitdag`. SPIT defines the graph; the runner executes commands and checks and records execution results.

## Command definitions and the bound DAG

See [binding and output](architecture/binding.md) for the internal representation, path binding, and command expansion. The [DAG format manual](spitdag.md) owns the file contract.

## Adding a feature

See [extending SPIT](architecture/extending.md) for ownership, lowering, invariants, and tests.

## Performance

See [performance](architecture/performance.md) for the data layout and scaling rules.
