# Architecture boundary

The pipeline definition is a reusable graph template. It names product families and operation invocations, but contains no concrete source artifact instances. `Pipeline` is the internal representation of that template.

`SourceInventory` is a separate internal input. It contains `SourceRecord` values identified by product name and complete entity binding, plus optional observed contexts. A dataset indexer may construct this structure directly or emit the small inventory text format. For a small example, `parse_document` can read pipeline and inventory sections from one file while returning separate `Pipeline` and `SourceInventory` values. The core does not know how an indexer found those records.

`resolve(&pipeline, &inventory)` checks the declarations and inventory, then enumerates jobs, logical artifact instances, and dependencies. A coverage rule counts a source family within each observed context; it does not prescribe the number of contexts in the dataset. This is how a missing required image in an observed session can fail without requiring a global subject or session count.

Symbolic type checking is separate from entity matching and is optional. Omitted product or operation types are represented by `TypeExpr::Unknown`. Each concrete job receives a fresh substitution map for its type variables. Known inputs unify with the signature, and the inferred output type is substituted into the job's output artifact. A known conflict fails resolution. `Unknown` leaves compatibility undecided and unresolved variables are erased to `Unknown` at the output boundary, preventing local variables from leaking into another job. Names, dimensions, and cardinality continue to drive resolution when every type is unknown.

The resolved DAG has logical identities only. A future physical binding layer will map each concrete source and output identity to a path, then verify the properties required to run. Execution backends will consume the resolved and physically bound DAG. Neither physical discovery nor command execution is part of the resolver.

## Command definitions and Bash backend

An operation's logical signature defines its named input and output ports, types, and shape/cardinality rules. Its executable implementation is a separate declaration keyed by operation name. For example, a `register` implementation may provide the ordered argument template `flirt -in {moving} -ref {reference} -out {registered}`. The author supplies the executable name, flags, argument order, and placeholders. SPIT must not infer a command-line interface from the logical signature.

The intended flow is:

```text
pipeline text -> logical resolution and validation -> concrete DAG
concrete DAG + physical bindings -> bound DAG
bound DAG + operation implementations -> argument expansion -> Bash script
```

Each placeholder must refer to a declared input or output port and be filled from that job's resolved artifact identities after path binding. Missing paths, unknown placeholders, and an unexpressible `many` argument expansion should fail before emitting Bash. The backend must quote resolved arguments for Bash and preserve declared argument order. Command templates cannot influence matching, grouping, type checks, or job count. A shell function may be an explicitly declared executable target if the generated script also makes its definition available; this is execution configuration, not a resolver rule.

An operation may have multiple named output ports. Each resolved invocation instance produces one job containing all its outputs; it must not be expanded into duplicate command invocations. Each output port binds to a distinct named product and receives its own logical identity, type, and declared shape. Downstream consumers of any of those products depend on the same producing job. The physical binding pass must provide a destination for every output, and execution validation must check the full output set. This models commands such as multi-tissue response estimation, which emit several artifacts together.

Multiple outputs, named ports in user-facing operation declarations, physical binding, command-template syntax, and Bash generation are not implemented yet. The current model stores only one output per operation and job, and the text parser assigns positional input names such as `input1` and `input2`. Supporting named output bindings and user-authored input port names must come before readable placeholders such as `{moving}`, `{reference}`, and `{registered}`.

The `examples/basic.spit` document embeds a small inventory for a one-command demonstration; `examples/basic.sources` contains that inventory alone for the external-input route. They use imaging names as sample data; `src/` contains no imaging-specific types, operations, or assumptions.
