# Keywords and records

This is a quick lookup for each statement or section header. A keyword's position and file matter: `path:` in a pipeline and in a recipe cover different products. The linked sections give validation rules and longer examples.

## Pipeline keywords

These go in a `.spit` file. Sources, imports, sidecar groups, and the dimension order belong at the top level. A stage may contain operations, commands, steps, and its own `path:` and `ext:` defaults.

| Keyword | Form | Meaning | Details |
| --- | --- | --- | --- |
| `source` | `source image : Image .nii.gz [sub, run]` | Declare an existing product family; type, extension, and dimensions are optional. | [Products](../language-reference.md#products-and-dimensions) |
| `sidecars` | `sidecars photo [sub, shot]:` | Declare source members with one identity and path stem, differing by extension. Members are indented `source` lines. | [Sidecar files](../language-reference.md#sidecar-files) |
| `dimensions` | `dimensions [model, config, seed]` | Set the pipeline-wide order when source declarations do not order dimensions later combined in a product. | [Dimension order](../language-reference.md#dimension-order) |
| `operation` | `operation align(image: Image) -> Image` | Declare input and output ports. Names of operations are defined by the pipeline author. | [Operations](operations.md) |
| `command` | `command align: tool {image} {@output}` | Give an operation its executable and arguments. | [Command templates](operations.md#command-templates) |
| `verify` | `verify align: inspect {image}` | Add an input check the runner executes before the operation's command. | [Verification](operations.md#verification-and-artifact-checks) |
| `check` | `check nonempty: test -s {@path}` | Declare a reusable test of one artifact. Attach it with `@ check(...)`. | [Checks](../language-reference.md#checks) |
| `stage` | `stage preprocess:` | Group indented steps and optionally give them path or extension defaults. Stages can nest. | [Stages](../language-reference.md#stages) |
| `use` | `use lib.spit as lib` or `use clean from lib.spit` | Import definitions from another pipeline file; calls and steps remain local. `from` selects names and `as` qualifies them. | [Reuse](../language-reference.md#reuse-definitions) |
| `path` | `path: out/{@product}` or `path image: in/{sub}.nii.gz` | Set a default path or a product's own path. A `path:` within a `sidecars` block gives its stem. | [Paths](../language-reference.md#paths) |
| `ext:` | `ext: .nii.gz` | Supply an extension for outputs whose operation declares none when a default path needs one. | [Extensions](../language-reference.md#extensions) |
| Step assignment | `cleaned = clean(image)` | Call an operation and name its output product; a multi-output call assigns one name per output. | [Calls](operations.md#call-an-operation) |

`many` is a port modifier, and `beside` is an output modifier; see [operation forms](operations.md#input-and-output-forms). `where`, `same`, `vary`, and `each` are input selectors, not standalone statements; see [selectors](selectors.md).

## Recipe keywords

These go in a `.spitin` file. A recipe names one dataset and its `.spit` pipeline; it cannot declare operations or steps.

| Keyword | Form | Meaning | Details |
| --- | --- | --- | --- |
| `pipeline` | `pipeline analysis.spit` | Name the pipeline relative to the recipe's folder. | [Recipes](../language-reference.md#recipes) |
| `root` | `root data` | Name the dataset folder relative to the recipe's folder; `root .` means that folder. | [Recipes](../language-reference.md#recipes) |
| `path` | `path image: raw/{sub}.nii.gz` or `path: raw/{@product}` | Give source path rules or a default for sources. It may name a `sidecars` group without a pipeline stem. | [File ownership](../language-reference.md#which-file-a-line-belongs-in) |
| `discover` | `discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}` | Read observed context bindings from directories. `from dirs` introduces the directory pattern. | [Discovery](../language-reference.md#discover-contexts-from-directories) |
| `exclude` | `exclude image[sub=02]` or `exclude from qc/excluded.csv` | Remove named artifacts or groups without deleting their files; the `from` form reads a CSV. | [Exclusion](../language-reference.md#exclude-named-artifacts) |
| `drop` | `drop [sub] where sessions count<2` | Remove every group satisfying a condition. | [Drop](../language-reference.md#drop-groups-that-fail-a-criterion) |
| `require` | `require image count=1 per [sub, ses]` | Fail when a retained group lacks the required artifacts or values. | [Constraints](../language-reference.md#constraints) |

A recipe can also contain `sources:` and `contexts:` records, using the same identity syntax as a `.spitout`. `exclude` runs before `drop`, and `require` checks what those rules leave, regardless of line order. See [recipe conditions](selectors.md#recipe-conditions).

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
