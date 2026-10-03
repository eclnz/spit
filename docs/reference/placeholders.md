# Placeholders

A placeholder is filled when SPIT binds a product to a path or a job to a command. The same braces can mean different things in a path, command, or check. A literal brace is written `{{` or `}}`.

## Path placeholders

| Placeholder | Example result | Meaning |
| --- | --- | --- |
| `{sub}`, `{run}`, etc. | `01` | That artifact's value for the named dimension. |
| `{@product}` | `aligned` | Product name; an imported `alias::name` writes `alias.name`. |
| `{@entities}` | `sub=01__run=2` | All dimensions in pipeline order, or `global` with none. |
| `{@labels}` | `sub-01_run-2` | All dimensions as `key-value` labels in pipeline order. |
| `{@stage}` | `preprocess/align` | Derived product's stage path, one directory per level. |

For example, `path: out/{@product}/{@entities}` writes separate products and identities beneath `out/`. In path rules, `[text]` keeps that text only if its placeholders have values for the product, as in `sub-{sub}[/ses-{ses}]`. An ungrouped `{@stage}` is an error for a product outside a stage. See [paths](../language-reference.md#paths) for optional groups, escaping, and path validation.

## Command placeholders

| Placeholder | Used in | Meaning |
| --- | --- | --- |
| `{input}` | `command`, `verify` | The artifact path bound to the named input port. A `many` port expands to one argument per member. |
| `{image}` | `command` | The path of a named output port called `image`. |
| `{@output}` | `command` | The path of a single unnamed output. |
| `{image.dir}` | `command` | Folder containing the named output's path; `.` at the dataset root. |
| `{image.stem}` | `command` | Output file name without its declared extension, for a tool that adds the extension itself. |
| `{@output.dir}`, `{@output.stem}` | `command` | The corresponding folder or stem for a single unnamed output. |

Only outputs have `.dir` and `.stem`. A `verify` command uses input ports only. Every command output must be referenced by its path, directory, or stem, except an output declared `beside` another. A `many` input placeholder must occupy a whole argument. See [command templates](operations.md#command-templates).

## Check placeholders

| Placeholder | Meaning |
| --- | --- |
| `{@path}` | The one artifact a declared `check` tests; the check command must use it. |
| `{param}` | The argument passed when the check is attached, such as `{n}` in `check ndim(n): check_ndim {@path} {n}`. |

Checks may read only that artifact and their declared parameters, so they add no job dependency. See [checks](../language-reference.md#checks).
