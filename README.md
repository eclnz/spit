# SPIT — Simple Pipeline in Text

SPIT compiles a reusable logical pipeline against an observed source inventory to produce a concrete DAG. Product names and entity bindings identify logical artifacts; filenames are not part of their identity. The Rust core treats all product types, operation names, and dimensions as user-defined symbols.

## Run the example

The [pipeline definition](examples/basic.spit) and [source inventory](examples/basic.sources) are separate inputs:

```sh
cargo run -- check examples/basic.spit --sources examples/basic.sources
cargo run -- dag examples/basic.spit --sources examples/basic.sources
```

`check` prints `Pipeline valid.` and the number of resolved jobs. `dag` prints the [text DAG](basic.dag.txt), including inputs, outputs, and dependencies. After `cargo build`, use `target/debug/spit` in place of `cargo run --`.

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

- **Pipeline definition:** product families, symbolic types, dimensions, operation contracts, invocations, and optional coverage constraints. It contains no individual source artifacts.
- **Source inventory:** the observed source artifact identities and optional entity contexts. It contains no operation definitions, types, or paths. An external indexer or manifest generator can create it.
- **Resolver:** validates the inventory against product declarations, applies coverage constraints, checks local types and shapes, and expands all jobs and dependencies. It does not inspect files.
- **Physical binding:** a later pass will map logical source identities to paths and verify files. This pass is not implemented yet.

Changing the inventory can change the number of jobs without changing the pipeline definition. SPIT assumes no fixed number of subjects, sessions, or scans.

## Small text format

The pipeline file has `products:`, `operations:`, `pipeline:`, and optional `constraints:` sections. For example:

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

An inventory file contains `sources:` and optional `contexts:` sections:

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

## Resolution rules

- Artifact types are symbolic names checked for exact equality at each input and output.
- Each source record must bind exactly the declared dimensions of its product. The product name and bindings must be unique.
- For a preserve operation, the first `one` input drives one output per artifact. Later `one` inputs match on shared dimensions. Zero matches is missing; multiple matches is ambiguous. A unique later input must be no more specific than the driving artifact.
- A `many` operation has exactly one input and requires `@ vary(dimension)` in its invocation. The resolver groups its input family by every other dimension and removes the varied dimension from each output identity.
- Coverage rules check counts per observed group. `count=1` requires exactly one; `count>=1` requires at least one. They do not assert a total number of groups.
- Invocation outputs have one producing invocation. The resolver sorts invocations by dependencies, detects cycles, and rejects duplicate artifacts.

Run the tests with `cargo test`. The [Rust construction example](examples/basic.rs) remains an internal API demonstration; pipeline authors use `.spit` text files.

## Current limits

SPIT does not yet discover source files, bind logical artifacts to paths, verify physical files, or execute jobs. It also does not implement generic semantic types or the broader shape algebra from the full design brief. The present text parser and resolver remain intentionally small while their boundaries are tested.
