# SPIT — Simple Pipeline in Text

SPIT v0.1 reads a small declarative text file, checks it, and expands logical artifact families into a concrete DAG. A product name identifies a family; the name plus complete entity bindings identifies one logical artifact instance. These instances are not necessarily files.

Try the [example pipeline](examples/basic.spit):

```sh
cargo run -- check examples/basic.spit
cargo run -- dag examples/basic.spit
```

After building, the equivalent commands are `spit check examples/basic.spit` and `spit dag examples/basic.spit`. `check` prints the validation result and job count. `dag` prints every job, input, output, and dependency. Neither command executes a pipeline job or inspects source files.

The text format has four sections:

```text
products:
    bold       : BOLD           [sub, ses, run]
    t1w        : T1w            [sub, ses]
    denoised   : DenoisedBOLD   [sub, ses, run]
    registered : RegisteredBOLD [sub, ses, run]
    mean_bold  : MeanBOLD       [sub, ses]

operations:
    denoise(BOLD) -> DenoisedBOLD
    register(DenoisedBOLD, T1w) -> RegisteredBOLD
    mean(many RegisteredBOLD) -> MeanBOLD

pipeline:
    denoised   = denoise(bold)
    registered = register(denoised, t1w)
    mean_bold  = mean(registered @ vary(run))

sources:
    bold[sub=01,ses=01,run=1]
    bold[sub=01,ses=01,run=2]
    t1w[sub=01,ses=01]
```

Section order is flexible. Blank lines and `#` comments are allowed. Names use letters, digits, and underscores, starting with a letter or underscore. Source values are single tokens. Operation inputs are positional in v0.1; the first single input drives output shape for a preserve operation.

## Resolution rules

- Artifact types are symbolic names checked for exact equality at each input and output.
- Source artifacts are supplied in the `sources:` block with complete entity bindings. Their product name and bindings must be unique.
- For a preserve operation, the first `one` input drives one output per artifact. Later `one` inputs match on shared dimensions. Zero matches is missing; multiple matches is ambiguous. A unique later input must be no more specific than the driving artifact.
- A `many` operation has exactly one input and requires `@ vary(dimension)` in its invocation. The resolver groups its input family by every other dimension, creates one job per group, and removes the varied dimension from each output identity. A group with one artifact is valid.
- Invocation outputs have one producing invocation. The resolver sorts invocations by dependencies, detects cycles, and rejects duplicate artifacts.
- Resolution is deterministic. The dry run renders entity bindings in each product's declared dimension order.

The [Rust construction example](examples/basic.rs) remains as an internal API demonstration and test harness. Pipeline authors use `.spit` text files.

## Findings from v0.1

The product family model maps cleanly to Rust structs and ordered maps. Local type checks are straightforward, and removing a dimension to form an aggregation key makes `vary(run)` a small, explicit shape rule. The first input convention keeps multi-input preserve operations simple, but a future API may need a named driving input. Ambiguity is most informative when it lists the concrete candidates from one named family. Binding inputs by product name means two differently named T1 products do not compete automatically; the caller chooses the family explicitly. The small text grammar covers the example without a general expression parser.
