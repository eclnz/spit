# Architecture boundary

The pipeline definition is a reusable graph template. It names product families and operation invocations, but contains no concrete source artifact instances. `Pipeline` is the internal representation of that template.

## Three steps

SPIT runs in three steps. Each has its own modules and its own command, and each passes a file to the next:

```text
                 ┌─────────────────────────────┐
  .spit ────────►│ 1. COMPILE PIPELINE         │──► checked pipeline
                 │    spit check               │
                 └─────────────────────────────┘
                        │ source declarations
                        ▼
  .spitin ─────►┌─────────────────────────────┐
  data/ ───────►│ 2. BUILD INPUTS             │──► .spitout
                │    spit inputs              │
                └─────────────────────────────┘
                        │
                        ▼
  pipeline ─────────►┌─────────────────────────────┐
  .spitout ─────────►│ 3. RESOLVE JOBS             │──► .spitdag, artifacts report
                     │    spit dag, spit artifacts │
                     └─────────────────────────────┘
```

1. **Compile** (`src/compile`, `src/paths/rules.rs`) checks everything the pipeline text determines: declarations, stages, each step's shape and inferred types, cycles, and path rules. It needs no inputs.
2. **Build inputs** (`src/inputs`) settles a dataset. It reads the pipeline only for its source declarations and path rules, and an `InputSpec`: the `discover`, `exclude`, `drop` and `require` rules, source paths, and any records of a `.spitin` recipe, with the pipeline its `pipeline` line names. It scans a root or takes the records written, applies `exclude` and then `drop`, checks `require`, and binds each source to its declared path. The result is a `SourceInventory`, rendered as a `.spitout` of source identities, any source path rules supplied by the recipe, and a `removed:` record of what the rules left out.
3. **Resolve jobs** (`src/resolver`, `src/paths/bind.rs`, `src/render.rs`) expands the compiled pipeline over that inventory, binds each artifact to its file, and expands each command into its arguments. It never sees a discovery, exclude, drop or require rule. Source files follow the pipeline's path rules or the rules settled once in the `.spitout`. The result is a `BoundDag`, written as a `.spitdag`.

A backend would turn a `.spitdag` into something that runs, reading nothing else: no pipeline, path rule or command template. SPIT has none at present.

Shared code (the model, parser, lowering, path templates, and the `BoundDag` in `src/spitdag.rs`) belongs to no step. Step 2 and step 3 each build on step 1, and neither uses the other. `diagnostics.rs` and `main.rs` run the steps in order. `tests/architecture.rs` checks direct module references for forbidden step dependencies. Every module is private: the library's API is what `lib.rs` re-exports.

The parser reads a document as one of two kinds. A pipeline may hold no `discover`, `exclude`, `drop` or `require` rule and no `sources:` or `contexts:` record, and `parse_pipeline` returns it as a `Pipeline`. A recipe holds rules, source paths and records, and `parse_input_spec` returns it as an `InputSpec`. `Pipeline` holds no input rules.

`SourceInventory` is a separate internal input. It contains `SourceRecord` values identified by product name and complete entity binding, optional observed contexts, and any source path rules settled from a recipe. A dataset indexer may construct this structure directly or emit the `.spitout` text format, where a flat record reads `image[sub=1,ses=1]` and a named context may nest its source products. The core does not know how an indexer found those records.

`resolve(&pipeline, &inventory)` checks the declarations and inventory, then enumerates jobs, logical artifact instances, and dependencies. Coverage rules belong to the input stage, which checks them before any job exists: `InputSpec::resolve` returns each `CoverageGap` with the sources it holds back. A coverage rule counts a source family within each observed context; it does not prescribe the number of contexts in the dataset. This is how a missing required image in an observed session can fail without requiring a global subject or session count. A rule can also require entity values, such as `run=1,2`, in each observed group.

`resolve` fails at the first job the inventory cannot complete. `resolve_artifacts_excluding` runs the same matching but keeps going, returning an `ArtifactReport`: every source, the DAG of jobs that can run, each `IncompleteJob` with its gaps, and each `CoverageGap`. A gap is either unmatched (a missing or ambiguous input, or a collection below its minimum) or blocked (an input that is itself incomplete). An incomplete job's outputs stay candidates for later steps, so a consumer is reported as blocked rather than silently producing no job; sources in a group that fails a coverage rule are held back the same way, passed in as the sources to exclude. `resolve` is this report's first unmatched gap: a blocked gap always follows the gap that blocks it. Only a step's driving artifacts create contexts, so a step whose driver is wholly absent for a context has no job, complete or not, to report there. `spit artifacts` renders the report and succeeds even when some artifacts are incomplete.

Each step has a driving input. For a preserve step it is the single-artifact input with the most dimensions, provided every other input is matched on a subset of them; the step's outputs take the driver's dimensions, and port order never changes the choice. For an aggregate step it is the one `many` input, grouped by every dimension except the one its `vary` names. Every other input is matched to each job's context and must leave exactly one artifact. Selectors narrow a binding: `where(dim=value)` keeps matching artifacts and removes the pinned dimension from matching, and `same(dim, ...)` matches on the listed dimensions alone. `each(dim, ...)` broadcasts a single input over dimensions the driver lacks: the step runs for every combination of the driver's groups with the values present in that input, the outputs gain those dimensions, and an input that broadcasts cannot drive. Artifact families, jobs, and collections follow the pipeline's dimension order, with runs of digits compared as numbers. Lowering sets that order (`src/order.rs`): each source orders its own dimensions, a `dimensions [...]` line declares the whole order, and every product a step makes has its dimensions sorted into it. A product holding two dimensions nothing orders, two sources that disagree, and a written output whose order differs are errors.

The parser reads the flow form: `source` declarations create product families, `operation` statements create reusable contracts, and assignments create invocations plus inferred intermediate product declarations. A pipeline can also group steps into stages: each `Invocation` records the `stage` whose block holds it, `Pipeline::stages` keeps each stage's optional default path rule, and every `Job` carries its step's stage. A nested stage's name is its path, such as `preprocess/combine`, and it inherits the nearest enclosing stage's default path. A stage is a grouping and a path scope, not a namespace; the resolver only checks that sibling stages do not depend on each other in a cycle, at every level, following products through steps written in an enclosing stage or outside every stage. The statements lower to a `Pipeline`.

Symbolic type checking is separate from entity matching and is optional. Omitted product or operation types are represented by `TypeExpr::Unknown`. The resolver first propagates types through the pipeline declarations in dependency order, so a known contradiction fails even when the inventory is empty. Each concrete job then receives a fresh substitution map for its type variables. Known inputs unify with the signature, and the inferred output type is substituted into the job's output artifact. A known conflict fails resolution. `Unknown` leaves compatibility undecided and unresolved variables are erased to `Unknown` at the output boundary, preventing local variables from leaking into another job. Later known inputs refine earlier partial bindings such as `Frame<Unknown>`. Names, dimensions, and cardinality continue to drive resolution when every type is unknown.

A product name identifies a result and can communicate its processing state; that state does not require a fresh nominal type. The ACT example uses `MRI<Kind,Space>` for image kinds and coordinate frames, so `raw_dwi` and `denoised_dwi` share `MRI<DWI,Acquired>`. A signature such as `mrtransform(MRI<K,S>, Transform<S,T>, MRI<N,T>) -> MRI<K,T>` works for both T1w and tissue products, with a fresh binding of `K`, `S`, and `T` at each invocation. Label images use a separate operation to select nearest-neighbor interpolation. Command implementations remain separate from these reusable logical contracts.

The current type engine has structural unification and type variables, but no subtype hierarchy or property refinements. `MRI<K,S>` can accept different image kinds by binding `K`; bare `MRI` is not presently a supertype of `MRI<T1w,Anatomical>`. A future domain-defined hierarchy could express such relationships explicitly, without baking imaging concepts into SPIT. Processing requirements such as distortion correction may need optional state properties and operation preconditions/effects; product names alone cannot prove those requirements. Unknown properties should remain unknown rather than incompatible by default. Registration mode and resampling interpolation are command choices that the current signature does not express; reusing a signature does not make every invocation's command arguments interchangeable.

The resolved DAG has logical identities, plus the file of each source whose record gave one. `bind_dag` binds every other source and output identity with the path templates and expands each command, giving the `BoundDag`. Neither physical discovery nor command execution is part of the resolver.

A DAG keeps each artifact once, in its `Artifacts` table, and jobs refer to artifacts by `ArtifactId`: a dataset has far more job inputs than artifacts. Every artifact of a product has the product's type, so the table keeps a product's name and type once, and each artifact is its product's number and its entities. What belongs to each artifact, such as a source's file or a bound path, is a column indexed by id. The `BoundDag` keeps the same table, with each artifact's path, and a command names a file by its artifact.

## Command definitions and the bound DAG

An operation's logical signature defines its input ports, its outputs, and its shape and cardinality rules. An operation with several outputs names them, as in `-> (wm: Response, csf: Response)`; one job writes them all, each output binds to its own product and path, and downstream consumers of any of them depend on that job. A single unnamed output is the port `output`. Its executable implementation is a separate `command` declaration keyed by operation name. For example, `operation register(moving: MRI<M,S>, reference: MRI<N,T>) -> Transform<S,T>` and `command register: flirt -in {moving} -ref {reference} -omat {output}` give each argument an explicit source and position. Unnamed ports retain positional names for compatibility. An aggregate's call names the dimensions it collects with `@ vary(run)`, so one operation can collect runs in one step and sessions in another; the resolver checks them against the output family's dimensions. SPIT does not infer a command-line interface from the logical signature.

The compilation flow is:

```text
pipeline text + .spitout -> logical resolution and validation -> concrete DAG
concrete DAG + path templates -> bound artifact paths
bound paths + operation commands -> argument expansion -> .spitdag
```

`spit check pipeline.spit --path-rules` reports whether each declared product uses an explicit rule, the default rule, or no rule. A source with no rule may receive one from a recipe; `spit check recipe.spitin --path-rules` lists the combined rules and marks those supplied by the recipe. Path templates are validated at compile time even for product families with no resolved jobs, and `dag` checks collisions among concrete paths. `--strict-paths` requires explicit rules for all products; without it, a default rule can cover multiple families. An output with no rule takes the built-in `out/{product}/{entities}`. A recipe's source rule is carried once in the `.spitout` for standalone resolution.

`spit dag --paths` reports each concrete job, its named input artifacts and output artifact, and their relative paths: the `.spitdag` without its commands.

`spit dag -o plan.spitdag` writes the `BoundDag`, and `spit dag --json` prints it, as schema version 3: `{"version":3,"generator":{...},"root":...,"external_inputs":[...],"targets":[...],"executables":[...],"jobs":[...]}`.

- `generator` names the program that wrote the file and its version, as `{"name":"spit","version":"0.2.0"}`.
- `root` is the absolute dataset folder every path is relative to: the `--root` folder, or the folder a recipe was scanned in. It is `null` when `dag` was given no folder, as with a `.spitout` and no `--root`, and the backend must then be told where the dataset is.
- `external_inputs` contains every job input whose producer is outside the graph: the sources. A backend can check they exist before it starts.
- `targets` contains every output no job reads: what a full run leaves behind. Every other output is an intermediate a backend may remove once its readers have run.
- `executables` lists, once each and sorted, the program each command and `verify` command runs: its first argument, when that is plain text. A backend can look for each on `PATH` before it runs anything.
- `jobs` lists every job after the jobs it depends on, so running them in the order given is always valid. Jobs retain their numeric IDs, which count up from 1 in that order.

Each job has `operation`, `stage` as an array of names from outermost to innermost (empty for an ungrouped job), `fingerprint`, `inputs` keyed by port name with arrays of artifacts (also for `one` ports), `outputs` keyed by port name, numeric `depends_on` IDs, the numeric `dependents` IDs of the jobs that read its outputs, `command`, and `verify`. An artifact has `product`, an `entities` object, a structured `type`, and its `path` relative to the dataset root: a known type is `{"name":"MRI","args":[{"name":"Parcellation","args":[]},{"name":"Diffusion","args":[]}]}`; an unknown type or argument is `null`. `command` is the job's argument list, or `null` for an operation with no command, and `verify` a list of them. Each argument is an array of parts, each a literal string or `{"path":...}`, joined without spaces, so a backend can place the root before every path and quote the rest.

The output is deterministic for a given pipeline, inventory, root and SPIT version. Job IDs are local to that resolution and can change when the inventory changes, so a backend that remembers jobs between runs keys them by their output paths, which no two jobs share. A job's `fingerprint` is 16 hexadecimal digits: a 64-bit FNV-1a hash of its operation, inputs, outputs, command and `verify` commands, as the `.spitdag` writes them. It ignores the job's ID, stage and neighbours, so it stays the same when other jobs come and go, and changes when anything the job reads, writes or runs does. A backend that stores each job's fingerprint beside its outputs can tell that a job must run again after its command changed, even if its outputs are newer than its inputs.

With `--root <directory>`, `dag` and `artifacts` also check that each source file needed by the resolved DAG exists as a file under that directory. Derived output paths are validated but are not expected to exist before execution. The field survey example has an integration test that creates empty files at its declared source paths and checks both complete and missing-input cases.

Each placeholder refers to an input or output port. A many port expands into individual quoted arguments in the collection's order, and its placeholder must occupy a complete argument. A `verify operation:` command uses the same placeholders for inputs only; a backend runs it before each of the operation's jobs and stops if it fails. It is where domain checks belong, such as comparing image grids with the tool that reads them. Unknown placeholders, path collisions, and an embedded many expansion fail before the `.spitdag` is written. Commands name executables expected on `PATH`; SPIT does not manage their installation. Command templates cannot influence matching, grouping, type checks, or job count. Checks of file contents are a task for domain-specific `verify` commands.

The `.spitdag` is specific to the supplied inventory; a new inventory requires a new `dag`.

`discover_sources(&pipeline, &rules, root)` walks a directory and matches source files against their path rules, reading entity values from the placeholders; a file that fits two sources' rules is an error. `discover name: [dimensions] from dirs pattern` also matches directories and adds their observed bindings as global contexts. It retains bindings by rule name so `require name count>=2 per [dimension]` checks that rule alone. `exclude` rules apply first, as contexts and files are found (`src/inputs/exclusions.rs`), so an excluded context expects no files and an excluded file may lie outside the discovered contexts. Every `drop` rule is then judged once against what was found, a file a context expects but lacks counting as absent, and the groups any rule rejects are removed at once, before expected source files are checked. Source products whose dimensions fit the rule expand over the remaining bindings and must have files at their paths. `spit inputs` writes named bindings as `contexts name:` sections. The recipe's folder is the root unless `--root` is given. A recipe that writes its own records is not scanned unless `--root` is given.

`InputSpec::resolve` runs step 2 for a recipe. It reads the logical pipeline only for its source products and path rules, then discovers contexts and source files, applies `exclude` and `drop` rules, and checks `require` rules, returning a plain inventory, with a record of what was removed, and what is missing. `spit inputs` writes that inventory only when nothing is missing. A `.spitout` carries source identities and any source path rules the recipe supplied, so it drives `dag` and `artifacts` without the recipe. Each source's bound path must agree with its rule. Named contexts in a `.spitout` whose rule is absent are kept as a record of where they came from. Given a recipe in place of a `.spitout`, `dag` and `artifacts` run step 2 in memory first, and `artifacts` then reports each coverage gap.

`examples/basic/basic.spit` is a small pipeline, with its recipe in `basic.spitin` and its inputs in `basic.spitout`. They use imaging names as sample data; `src/` contains no imaging-specific types, operations, or assumptions.
