# SPIT — Simple Pipeline in Text

SPIT compiles a reusable logical pipeline against an observed source inventory to produce a concrete DAG. Product names and entity bindings identify logical artifacts; filenames are not part of their identity. The Rust core treats all product types, operation names, and dimensions as user-defined symbols.

## Run the example

The [basic example](examples/basic.spit) packages a pipeline definition and a small source inventory in one text file:

```sh
cargo run -- check examples/basic.spit
cargo run -- dag examples/basic.spit
```

`check` prints `Pipeline valid.` and the number of resolved jobs. `dag` prints the [text DAG](basic.dag.txt), including inputs, outputs, and dependencies. After `cargo build`, use `target/debug/spit` in place of `cargo run --`.

The same inventory can be supplied explicitly from [basic.sources](examples/basic.sources):

```sh
cargo run -- dag examples/basic.spit --sources examples/basic.sources
```

`--sources` selects the external inventory when the document also contains one. A reusable pipeline file can omit `sources:` and `contexts:` entirely and receive the inventory only through `--sources`.

The [typed example](examples/typed.spit) shows inferred output types in the dry run:

```sh
cargo run -- dag examples/typed.spit --sources examples/typed.sources
```

For `Frame<Native>` and `Reference<Target>`, the example infers `CleanFrame<Native>` and `Transform<Native,Target>` without type annotations on its pipeline calls.

Larger examples exercise the same compiler with different graph structures:

| Example | Structure | Run |
| --- | --- | --- |
| [Branching](examples/branching.spit) | Two branches, shared policy, parallel aggregations, recombination | `cargo run -- dag examples/branching.spit` |
| [Observed groups](examples/rich_shapes.spit) | Several subjects and sessions, reference reuse, successive aggregations | `cargo run -- dag examples/rich_shapes.spit --sources examples/rich_shapes.sources` |
| [Nested aggregation](examples/complex.spit) | Partial typing, uneven groups, and three aggregation levels | `cargo run -- dag examples/complex.spit` |
| [MRtrix3 ACT](examples/mrtrix3_act.spit) | Raw DWI preprocessing, T1 registration, parcellation alignment, ACT tractography, SIFT2, connectomes | `cargo run -- dag examples/mrtrix3_act.spit --sources examples/mrtrix3_act.sources` |

The ACT file starts with raw DWI runs, native T1w, reverse phase-encoded b=0 images, and lookup tables. Its products share parameterized types such as `MRI<DWI,Acquired>`: product names distinguish processing states, while reusable operations describe compatible data kinds and coordinate spaces. For example, one `mean_b0` signature serves two stages, and one `mrtransform` signature serves T1w and parcellation images. SPIT resolves this logical graph; it does not yet run the tools, inspect files, or verify spatial alignment. [Exploration findings](docs/exploration.md) records what these larger pipelines exposed.

An indexer can supply an inventory on standard input, so pipeline authors need not list every scan by hand:

```sh
dataset-indexer | cargo run -- dag examples/basic.spit --sources -
```

`dataset-indexer` is illustrative; SPIT does not yet provide one. The `.sources` file is a fixture and a simple interchange format for a future indexer.

## Layers

```text
pipeline definition ──┐
                     ├──> generic resolver ──> concrete logical DAG
source inventory ─────┘                          │
                                                └──> later physical binding and execution
```

- **Pipeline definition:** product families, optional symbolic types, dimensions, operation contracts, invocations, and optional coverage constraints. The internal `Pipeline` value contains no individual source artifacts.
- **Source inventory:** the observed source artifact identities and optional entity contexts. It contains no operation definitions, types, or paths. An external indexer or manifest generator can create it.
- **Resolver:** validates the inventory against product declarations, applies coverage constraints, checks local types and shapes, and expands all jobs and dependencies. It does not inspect files.
- **Physical binding:** a later pass will map logical source identities to paths and verify files. This pass is not implemented yet.

Changing the inventory can change the number of jobs without changing the pipeline definition. SPIT assumes no fixed number of subjects, sessions, or scans.

## Small text format

The pipeline text has `products:`, `operations:`, `pipeline:`, and optional `constraints:` sections. For example:

```text
products:
    image      : Image      [subject, visit, repeat]
    reference  : Reference  [subject, visit]
    registered : Registered [subject, visit, repeat]
    average    : Average    [subject, visit]

operations:
    align(Image, Reference) -> Registered
    mean(many Registered) -> Average

pipeline:
    registered = align(image, reference)
    average = mean(registered @ vary(repeat))

constraints:
    require image count>=1 per [subject, visit]
    require reference count=1 per [subject, visit]
```

An inventory contains `sources:` and optional `contexts:` sections. These can appear in the same `.spit` document for an example, or in a separate file supplied with `--sources`:

```text
contexts:
    [subject=A,visit=1]

sources:
    image[subject=A,visit=1,repeat=1]
    image[subject=A,visit=1,repeat=2]
    reference[subject=A,visit=1]
```

Contexts and source records establish the observed groups for coverage checks. If a context is present but lacks a required artifact, a rule reports the missing count. Source records can also establish groups without explicit context rows. A group invisible to both the contexts and source records cannot be inferred as missing.

Section order is flexible. Blank lines and `#` comments are allowed. Names use letters, digits, and underscores, starting with a letter or underscore. Source values are single tokens. Operation inputs are positional in this prototype; the first single input drives output shape for a preserve operation.

Types are optional. The same shape rules work with an untyped declaration:

```text
products:
    raw       [subject, repeat]
    processed [subject, repeat]
    combined  [subject]

operations:
    process(one)
    combine(many)

pipeline:
    processed = process(raw)
    combined = combine(processed @ vary(repeat))
```

Add `: Type` to a product, or input and output types to an operation, whenever those checks are useful. An omitted type is `Unknown`: it participates in resolution and causes no mismatch by itself. When two known types conflict, resolution fails. Product names and dimensions still determine artifact identity and shape; types do not select a product in this version of the text language.

Run the complete [untyped example](examples/untyped.spit) with `cargo run -- dag examples/untyped.spit`.

## Resolution rules

- Artifact types, when supplied, are symbolic expressions. Named constructors and their argument structure must match; an operation's single-letter uppercase type variables unify across its ports and are instantiated separately for each job. Missing types become `Unknown`, which is compatible but does not establish a variable binding.
- Each source record must bind exactly the declared dimensions of its product. The product name and bindings must be unique.
- For a preserve operation, the first `one` input drives one output per artifact. Later `one` inputs match on shared dimensions. Zero matches is missing; multiple matches is ambiguous. A unique later input must be no more specific than the driving artifact.
- A `many` operation has exactly one input and requires `@ vary(dimension)` in its invocation. The resolver groups its input family by every other dimension and removes the varied dimension from each output identity.
- Coverage rules check counts per observed group. `count=1` requires exactly one; `count>=1` requires at least one. They do not assert a total number of groups.
- Invocation outputs have one producing invocation. The resolver sorts invocations by dependencies, detects cycles, and rejects duplicate artifacts.

Run the tests with `cargo test`. The [Rust construction example](examples/basic.rs) remains an internal API demonstration; pipeline authors use `.spit` text files.

## Current limits

SPIT does not yet discover source files, bind logical artifacts to paths, verify physical files, or execute jobs. Type checking has no subtyping, automatic coercion, strict mode, or graph-wide inference. Unresolved type variables become `Unknown` at a job output. Operations still have one output, and the broader shape algebra from the full design brief remains to be implemented.
