# Architecture boundary

The pipeline definition is a reusable graph template. It names product families and operation invocations, but contains no concrete source artifact instances. `Pipeline` is the internal representation of that template.

`SourceInventory` is a separate internal input. It contains `SourceRecord` values identified by product name and complete entity binding, plus optional observed contexts. A dataset indexer may construct this structure directly or emit the small inventory text format. For a small example, `parse_document` can read pipeline and inventory sections from one file while returning separate `Pipeline` and `SourceInventory` values. The core does not know how an indexer found those records.

`resolve(&pipeline, &inventory)` checks the declarations and inventory, then enumerates jobs, logical artifact instances, and dependencies. A coverage rule counts a source family within each observed context; it does not prescribe the number of contexts in the dataset. This is how a missing required image in an observed session can fail without requiring a global subject or session count.

The parser accepts both a sectioned form and a flow-first form. In the flow-first form, `source` declarations create product families, `require` statements create coverage rules, `operation` statements create reusable contracts, and assignments create invocations plus inferred intermediate product declarations. Both forms lower to the same `Pipeline`; the inventory remains a separate `SourceInventory` even when a document embeds a `sources:` section.

Symbolic type checking is separate from entity matching and is optional. Omitted product or operation types are represented by `TypeExpr::Unknown`. The resolver first propagates types through the pipeline declarations in dependency order, so a known contradiction fails even when the inventory is empty. Each concrete job then receives a fresh substitution map for its type variables. Known inputs unify with the signature, and the inferred output type is substituted into the job's output artifact. A known conflict fails resolution. `Unknown` leaves compatibility undecided and unresolved variables are erased to `Unknown` at the output boundary, preventing local variables from leaking into another job. Later known inputs refine earlier partial bindings such as `Frame<Unknown>`. Names, dimensions, and cardinality continue to drive resolution when every type is unknown.

A product name identifies a result and can communicate its processing state; that state does not require a fresh nominal type. The ACT example uses `MRI<Kind,Space>` for image kinds and coordinate frames, so `raw_dwi` and `denoised_dwi` share `MRI<DWI,Acquired>`. A signature such as `mrtransform(MRI<K,S>, Transform<S,T>, MRI<N,T>) -> MRI<K,T>` works for both T1w and tissue products, with a fresh binding of `K`, `S`, and `T` at each invocation. Label images use a separate operation to select nearest-neighbor interpolation. Command implementations remain separate from these reusable logical contracts.

The current type engine has structural unification and type variables, but no subtype hierarchy or property refinements. `MRI<K,S>` can accept different image kinds by binding `K`; bare `MRI` is not presently a supertype of `MRI<T1w,Anatomical>`. A future domain-defined hierarchy could express such relationships explicitly, without baking imaging concepts into SPIT. Processing requirements such as distortion correction may need optional state properties and operation preconditions/effects; product names alone cannot prove those requirements. Unknown properties should remain unknown rather than incompatible by default. Registration mode and resampling interpolation are command choices that the current signature does not express; reusing a signature does not make every invocation's command arguments interchangeable.

The resolved DAG has logical identities only. The Bash backend binds each concrete source and output identity with user-defined path templates, then expands commands into a script. The script checks file existence at runtime. Neither physical discovery nor command execution is part of the resolver.

## Command definitions and Bash backend

An operation's logical signature defines its input ports, output type, and shape/cardinality rules. Its executable implementation is a separate `command` declaration keyed by operation name. For example, `operation register(moving: MRI<M,S>, reference: MRI<N,T>) -> Transform<S,T>` and `command register: flirt -in {moving} -ref {reference} -omat {output}` give each argument an explicit source and position. Unnamed ports retain positional names for compatibility. An aggregate contract can name the removed dimension with `@ drop(run)`; the resolver checks it against the call's `@ vary(run)` and the output family dimensions. SPIT does not infer a command-line interface from the logical signature.

The compilation flow is:

```text
pipeline text -> logical resolution and validation -> concrete DAG
concrete DAG + path templates -> bound artifact paths
bound paths + operation commands -> argument expansion -> Bash script
```

`spit paths` reports whether each declared product uses an explicit rule, the default rule, or no rule. It validates template dimensions even for product families with no resolved jobs and checks collisions among concrete paths. `--strict-paths` requires explicit rules for all products during compilation; without it, a default rule can cover multiple families.

`spit bound-dag` reports each concrete job, its named input artifacts and output artifact, and their relative paths. This binds the DAG to paths without reading or expanding command templates.

With `--root <directory>`, SPIT also checks that each source file needed by the resolved DAG exists as a file under that directory. Derived output paths are validated but are not expected to exist before execution. The ACT example has an integration test that creates empty files at its declared source paths and checks both complete and missing-input cases.

Each placeholder refers to an input port or the output. A sole many port expands into individual quoted arguments in lexicographic entity-binding order, and its placeholder must occupy a complete argument. Missing path templates, unknown placeholders, path collisions, and an embedded many expansion fail before emitting Bash. The generated script invokes command executables available on `PATH`; SPIT does not manage their installation. Command templates cannot influence matching, grouping, type checks, or job count. The resulting script checks required source paths, creates output directories, and checks that each job produced its declared output. Source discovery and richer physical validation remain separate work.

The current model stores one output per operation and job. To support commands such as multi-tissue response estimation, a future invocation should produce one job with several named output ports. Each port would bind to a distinct product and path, and downstream consumers of any output would depend on that same job. Multiple outputs and named output ports are not implemented yet. The generated script is specific to the supplied inventory; a new inventory requires a new compile.

The `examples/basic/basic.spit` document embeds a small inventory for a sectioned pipeline example; `examples/basic/basic.sources` contains that inventory alone for the external-input route. They use imaging names as sample data; `src/` contains no imaging-specific types, operations, or assumptions.
