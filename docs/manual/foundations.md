# Foundations

## Terms

| Term | Meaning |
| --- | --- |
| Product | A named family declared by a source or created by a step. |
| Dimension | A named coordinate of a product family, such as `subject` or `run`. |
| Entity binding | The mapping from every dimension of a product to a textual value. |
| Artifact | One product plus one complete entity binding; its file or folder is named by a path rule. |
| Operation | A reusable ordered-port contract with a command or body of steps. |
| Step | A call that assigns product names to an operation’s outputs. |
| Job | One resolved instance of a step, with concrete inputs, outputs, and dependencies. |
| Context | An observed dimension binding used to discover or assess source coverage. |
| Runner / backend | A program that executes the runnable DAG. |

`image[sub=01,run=2]` identifies an artifact; `Image` in `source image : Image [sub, run]` is a type. A path is not an identity. Entity values match as text: `01` differs from `1`, and `A` differs from `a`. Natural ordering can place numeric portions in numerical order without making their identities equal.

## Grammar notation

Grammar fragments use `=` for a production, `|` for alternatives, quoted text for literal syntax, `?` for an optional item, `*` for zero or more items, and `+` for one or more. Parentheses group alternatives. Uppercase `NEWLINE`, `INDENT`, and `DEDENT` denote line and block boundaries. A name such as `type` or `path-template` refers to the corresponding component specification. Grammar fragments describe structure; the accompanying constraints also apply.

```ebnf
identifier = (letter | "_") (letter | digit | "_")*
qualified-name = identifier ("::" identifier)*
dimension-list = "[" identifier ("," identifier)* "]"
```

Here `letter` and `digit` are ASCII letters and digits. `qualified-name` is used where imported names are accepted; declaration names are not arbitrary paths. Empty list items and trailing commas are rejected. A product without dimensions omits its dimension list. Types have their own capitalization and variable rules under [types](types.md).

## Text and lines

Source text is UTF-8. A leading UTF-8 byte order mark is ignored. Statements are line-oriented: an operation signature, assignment, or command is written on one physical line. There is no general declaration continuation or multiline port-list syntax. Blank lines and comment-only lines do not close a block.

An unquoted, unescaped `#` starts a comment only at the start of a word. Thus `tool --color=#fff` keeps the `#`, while `tool # explanation` begins a comment. In `word# note` the hash stays part of the word and produces a warning because it resembles a comment. Quotes and escapes affect comment recognition; [command argument rules](operations.md#operations-and-commands) specify how quoted arguments are parsed.

A stage or operation body begins with a header ending in `:`. Direct lines within a block use one consistent indentation, greater than the header’s; a line back at the header’s indentation or less closes that block. Stages may nest. An operation body contains steps, not arbitrary declarations. Source declarations, imports, and a pipeline-wide dimension declaration belong at the top level.

## Names and scope

Products, operations, and dimensions have separate names. An operation name is global even when its declaration appears inside a stage. A stage is a grouping and default scope, not a namespace. An import alias introduces `alias::name` for imported definitions. Product and operation names may coincide, but `check` warns because the result is less clear.

An operation must be declared before a call to it. Top-level steps are checked as a dependency graph and may refer to products made by later steps; cycles are rejected. Operation bodies have a stricter local scope: they read their input ports and products of earlier body steps, and each named output is assigned once. Their private intermediates cannot be read from the caller.

## Validation phases

1. **Compilation** checks declarations, types, dimension compatibility, cycles, command templates, and symbolic path rules without reading dataset files.
2. **Input settlement** scans files or reads source records, applies recipe rules, and checks coverage.
3. **Resolution and binding** choose actual input artifacts, create jobs, check concrete paths and required source existence, and bind command arguments.
4. **Execution** belongs to a runner, which checks file contents through declared checks and verification and requires declared outputs.

A syntactically accepted statement can fail a later phase. For example, a `same` selector can be dimensionally valid but ambiguous in a particular inventory. Required file contents cannot be proven by a type annotation.

The resolved output is deterministic for the same pipeline, inventory, root, and SPIT version. Changing the inventory can change local job IDs. [DAG fingerprints](dag.md#fingerprint) identify each job’s work, rather than those IDs or file contents.

## Minimal valid and invalid forms

```spit
source raw [subject]
operation copy(input)
result = copy(raw)
```

This is a valid logical pipeline; the missing command is a warning and source paths are needed before binding concrete source artifacts.

Invalid: `source 2raw [subject]` starts an identifier with a digit. Invalid: `source raw [subject,]` has an empty list item. Invalid: placing `exclude raw[subject=A]` in a pipeline mixes dataset policy into the reusable graph; it belongs in a recipe.
