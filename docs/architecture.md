# Architecture boundary

The pipeline definition is a reusable graph template. It names product families and operation invocations, but contains no concrete source artifact instances. `Pipeline` is the internal representation of that template.

`SourceInventory` is a separate input. It contains `SourceRecord` values identified by product name and complete entity binding, plus optional observed contexts. A dataset indexer may construct this structure directly or emit the small inventory text format. The core does not know how the indexer found those records.

`resolve(&pipeline, &inventory)` checks the declarations and inventory, then enumerates jobs, logical artifact instances, and dependencies. A coverage rule counts a source family within each observed context; it does not prescribe the number of contexts in the dataset. This is how a missing required image in an observed session can fail without requiring a global subject or session count.

The resolved DAG has logical identities only. A future physical binding layer may attach paths and inspect files. Execution backends will consume a fully resolved and validated DAG. Neither physical discovery nor command execution is part of the resolver.

The `examples/basic.spit` pipeline and `examples/basic.sources` inventory demonstrate the boundary. They use imaging names as sample data; `src/` contains no imaging-specific types, operations, or assumptions.
