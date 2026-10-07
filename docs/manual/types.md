# Types

Types annotate logical compatibility. They do not prove file contents, choose commands, or add runtime validation. Type names are supplied by the author; there is no built-in domain type catalog.

## Optional types

Types are additive. You can leave them out, add them to selected products and operations, or type the whole pipeline. Known mismatches fail; missing type information does not.

In operation signatures, a single capital letter such as `S` is a local type variable. Use a `$` prefix for longer names, such as `$SourceSpace` or `$Kind`. An unprefixed name such as `World` is a concrete type; every type name starts with a capital letter. Variables are allowed in operation signatures, not product declarations:

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```

## Syntax

```ebnf
type = named-type | variable
named-type = type-name ("<" type ("," type)* ">")?
variable = capital-letter | "$" type-name
```

Type names start with a capital letter. A single capital letter in an operation signature is a local variable; use a longer unprefixed name for a concrete type. Variables may appear within nested type arguments. Product declarations cannot declare local signature variables.

## Compatibility

Known types unify structurally: their names and number of arguments must match, and each corresponding argument must unify. There is no subtype hierarchy or property refinement. Bare `MRI` is therefore not a supertype of `MRI<Anatomical,World>`.

Each call receives fresh variable bindings. Repeated variables impose equality: `operation compare(a: Frame<S>, b: Frame<S>) -> Frame<S>` requires known input arguments to agree. Unknown information does not cause a mismatch; later known inputs can refine a partial binding. An output variable left unresolved becomes unknown rather than leaking into another call.

Known contradictions are checked before reading an inventory, including with an empty dataset. Types do not affect identity, collection order, path discovery, or how many jobs are made.

## Examples

```spit
source scan : MRI<Anatomical,World> [sub]
operation preserve(image: MRI<$Kind,$Space>) -> MRI<$Kind,$Space>
result = preserve(scan)
```

`result` has `MRI<Anatomical,World>`. The signature may be reused in another call with different bindings.

Invalid: passing `Table` to an input declared `Image`. Invalid: two known arguments `Frame<World>` and `Frame<Scanner>` for the repeated `S` above. An untyped argument alone is not an error and does not prove either type.
