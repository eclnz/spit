# Keywords and records

This is a quick lookup for each statement or section header. A keyword's position and file matter: `path:` in a pipeline and in a recipe cover different products. The linked sections give validation rules and longer examples.

## Pipeline keywords

These go in a `.spit` file. Sources, imports, and the dimension order belong at the top level. A stage may contain operations, commands, steps, and its own `path:`, `ext:`, and `check:` defaults.

| Keyword | Form | Meaning | Details |
| --- | --- | --- | --- |
| `source` | `source image : Image .nii.gz [sub, run]` | Declare an existing product family; type, extension, and dimensions are optional. | [Products](../language-reference.md#products-and-dimensions) |
| `dimensions` | `dimensions [model, config, seed]` | Set the pipeline-wide order when source declarations do not order dimensions later combined in a product. | [Dimension order](../language-reference.md#dimension-order) |
| `operation` | `operation align(image: Image) -> Image` | Declare input and output ports. An operation may have a command or an indented body of steps. | [Operations](operations.md) |
| `command` | `command align: tool {image} {@output}` | Give an operation its executable and arguments. | [Command templates](operations.md#command-templates) |
| `verify` | `verify align: inspect {image}` | Add an input check the runner executes before the operation's command. | [Verification](operations.md#verification-and-artifact-checks) |
| `check` | `check nonempty: test -s {@path}` | Declare a reusable test of one artifact. Attach it with `@ check(...)`. | [Checks](../language-reference.md#checks) |
| `check:` | `check: nonempty, ndim(3)` | Set checks for every output in the file or stage; `!name` removes an inherited default. | [Default checks](../language-reference.md#default-checks) |
| `stage` | `stage preprocess:` | Group indented steps and optionally give them path or extension defaults. Stages can nest. | [Stages](../language-reference.md#stages) |
| `use` | `use lib.spit as lib` or `use clean from lib.spit` | Import definitions from another pipeline file; top-level calls and steps remain local. `from` selects names and `as` qualifies them. | [Reuse](../language-reference.md#reuse-definitions) |
| `path` | `path: out/{@product}` or `path image: in/{sub}.nii.gz` | Set a default path or a product's own path. A companion source takes the main source's path stem. | [Paths](../language-reference.md#paths) |
| `ext:` | `ext: .nii.gz` | Supply an extension for outputs whose operation declares none when a default path needs one. | [Extensions](../language-reference.md#extensions) |
| Step assignment | `cleaned = clean(image)` | Call an operation and name its output product; a multi-output call assigns one name per output. | [Calls](operations.md#call-an-operation) |

`many` is a port modifier, and `beside` can declare a source or output companion; see [operation forms](operations.md#input-and-output-forms) and [sidecar files](../language-reference.md#sidecar-files). `where`, `same`, `vary`, and `each` are input selectors, not standalone statements; see [selectors](selectors.md).

## Recipe keywords

These go in a `.spitin` file. A recipe names one dataset and its `.spit` pipeline; it cannot declare operations or steps.

| Keyword | Form | Meaning | Details |
| --- | --- | --- | --- |
| `pipeline` | `pipeline analysis.spit` | Name the pipeline relative to the recipe's folder. | [Recipes](../language-reference.md#recipes) |
| `root` | `root data` | Name the dataset folder relative to the recipe's folder; `root .` means that folder. | [Recipes](../language-reference.md#recipes) |
| `path` | `path image: raw/{sub}.nii.gz` or `path: raw/{@product}` | Give source path rules or a default for sources. Name the main source when its companions are declared `beside` it. | [File ownership](../language-reference.md#which-file-a-line-belongs-in) |
| `discover` | `discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}` | Read observed context bindings from directories. `from dirs` introduces the directory pattern. | [Discovery](../language-reference.md#discover-contexts-from-directories) |
| `exclude` | `exclude image[sub=02]`, `exclude [sub] where sessions count<2`, or `exclude from qc/excluded.csv` | Remove named artifacts or groups, or groups meeting a condition; the `from` form reads a CSV. | [Exclusion](../language-reference.md#exclude-groups-that-meet-a-condition) |
| `require` | `require [sub, ses] where image count=1` | Fail when a retained group lacks the required artifacts or values. | [Constraints](../language-reference.md#constraints) |

A recipe can also contain `sources:` and `contexts:` records, using the same identity syntax as a `.spitout`. Named exclusions apply first, then conditional exclusions, then `require`, regardless of line order. See [recipe conditions](selectors.md#recipe-conditions).

## Inventory headers

`spit inputs` writes a `.spitout`; a person or indexer may write one too. The headers introduce records, not operations or recipe rules.

| Header | Form | Meaning |
| --- | --- | --- |
| `root` | `root ../data` | Dataset folder relative to this `.spitout`. Without it, `dag` does not check source files on disk. |
| `source_paths:` | `source_paths:` followed by `image: data/{sub}.nii.gz` | Path rules supplied by the recipe, retained for `dag`. |
| `contexts:` | `contexts:` or `contexts sessions:` | Observed bindings; the named form records one `discover` rule's contexts. |
| `sources:` | `sources:` followed by `image[sub=01]` | Settled source identities. A record's path comes from its source rule. |
| `removed:` | `removed:` followed by removed identities and rule details | Record of what recipe rules removed; resolving jobs removes nothing more. |

See [inputs](../language-reference.md#inputs) for nested records, `source_paths:`, removal details, and the exact `.spitout` format.
