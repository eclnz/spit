# Command line reference

SPIT checks a pipeline, settles a dataset's inputs, and resolves jobs. Run `spit help` or `spit help <command>` for the binary's help. The [user guide](guide/pipelines.md) teaches the files it reads.

`spit --version` (`-V`) identifies the compiler and prints its release version. `spit --dag-version` prints the `.spitdag` format version it writes. Compare it with `spit-bash --dag-version` to check whether the runner can read that format.

| Command | Purpose | Common invocation |
| --- | --- | --- |
| `check` | Validate a pipeline, recipe, input inventory, or tools file | `spit check pipeline.spit --path-rules` |
| `inputs` | Scan sources and settle recipe rules | `spit inputs dataset.spitin -o dataset.spitout` |
| `dag` | Resolve and inspect concrete jobs | `spit dag dataset.spitin` |
| `artifacts` | Explain complete and incomplete artifacts | `spit artifacts dataset.spitin` |

## Input forms

`check` accepts one `.spit`, `.spitin`, `.spitout`, or `.spitenv` file; a `.spitenv` is checked alone, and a recipe checks the `.spitenv` its [`tools from`](guide/operations.md#tools) line names against its pipeline. `inputs` accepts a `.spitin` recipe, or a `.spit` pipeline with an inline `root` or `--root <directory>`. `dag` and `artifacts` accept those same forms, or a pipeline with a `.spitout`. A recipe names its pipeline and either declares a root or inherits that pipeline's root. Inline roots are relative to their declaring file; `--root` is relative to the working folder and is an error when the pipeline already declares a root. `--output-root <directory>` names the [output root](guide/datasets.md#output-root) the same way, for a pipeline that has no `output` line, and is an error with a recipe or a `.spitout`. A `.spitout` does not name its pipeline, so pass the pipeline separately.

<!-- spit-doc: fragment -->

```sh
spit dag dataset.spitin
spit dag pipeline.spit dataset.spitout
spit dag pipeline.spit --root data
spit dag pipeline.spit --root data --output-root results
```

`spit dag pipeline.spit --tree` reads the pipeline and its imports only. A compact overview names the products connecting stages. Within each stage, lines connect product producers to their consumers; matching product names link stage boundaries. Each distinct input has its own arrow into a boxed multi-input operation; simple chains use parentheses. Reused products branch along one live line, and `╪` marks a crossing without a join. Connected diagrams pack operations across a 120-column target and fold back through continuous product lines. Very wide labels or live product frontiers use the compact vertical layout. Output branches appear after operations. Calls to operations with bodies remain single nodes, followed by one diagram per used body, including nested bodies. Unused sources are listed. This view accepts no dataset or other flags and leaves resolved-job output unchanged.

Use `-` in place of a `.spitout` to read it from standard input. A printed inventory names its roots as absolute paths, so `spit inputs dataset.spitin | spit dag analysis.spit -` plans as the recipe does. A recipe and a `.spitout` can be used to inspect an existing dataset without writing a DAG file.

## Inspection and output flags

| Flag | With | Effect |
| --- | --- | --- |
| `--path-rules` | `check` | List each product's effective path rule and its source. |
| `--calls` | `check` on a `.spit` | List each call to an operation carried out by steps: the call, its line and stage, the file that declares the operation with the file's blob id, then each step with the line of the body that writes it, a nested call under its caller. With `--json`, print `{"diagnostics":[...],"calls":[...]}`, one object per call with its `steps`, and a nested call's `parent` the `id` of its caller. |
| `--unmatched` | `inputs` | List dataset files no source path rule reads. |
| `--tree` | `dag` on one `.spit` | Show product/step topology without reading dataset inputs. |
| `--counts` | `dag` | Print a job count for every step and a total; a call's steps are listed under it, with the call's jobs in all. |
| `--paths` | `dag` | Print artifact paths in the plan. |
| `--commands` | `dag` | Print the commands the runner would execute, with each job's `needs:`, `verify:` and `check:` lines, and a `from:` line naming the calls a job is nested in, outermost first, and the body's step. |
| `--partial` | `dag` | Keep jobs with complete inputs and record left-out outputs. |
| `--by-target` | `artifacts` | Group incomplete artifacts under the final targets they prevent. |
| `-o <file>` | `inputs`, `dag` | Save a `.spitout` or `.spitdag`, respectively. |
| `--json` | `check`, `dag` | Print structured diagnostics or the DAG. |
| `--stdin` | `check` | Read the file text from standard input, retaining its path for relative references. |
| `--hovers` | `check --json` | Include editor hovers and documentation for language words. |

Plain `dag` prints step counts; use `--commands` to list commands, `--jobs` to list job inputs and outputs, or `--paths` to print artifact paths. `--counts` can precede either job view. `--paths` and `--commands` can print together. With `-o`, an explicit `--counts`, `--commands`, `--jobs` or `--paths` also prints, from the same plan the DAG file saves, and plain `-o` prints nothing. `--json` is a separate output form. `check --calls` cannot combine with `--path-rules` or `--hovers`.

## Reading `check --json`

`check --json` prints one JSON object on standard output and nothing on standard error, whatever it finds in the file, a file it cannot read included. A tool decodes standard output whenever it is not empty:

<!-- spit-doc: output
spit check copy.spit --json && echo "exit $?"
--- copy.spit
source reading : Text .txt [site]
path reading: readings/{site}.txt
default path: {@product}/{site}
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(reading)
-->

```text
{"diagnostics":[],"paths":[{"product":"copied","line":6,"path":"copied/{site}.txt"}]}
exit 0
```

Warnings alone exit 0, and any error exits 1:

<!-- spit-doc: output
spit check unused.spit --json && echo "exit $?"
spit check nopath.spit --json || echo "exit $?"
--- unused.spit
source reading : Text .txt [site]
source notes : Text .txt [site]
path reading: readings/{site}.txt
path notes: notes/{site}.txt
path copied: copied/{site}.txt
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(reading)
--- nopath.spit
source reading : Text .txt [site]
path reading: readings/{site}.txt
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(reading)
-->

```text
{"diagnostics":[{"severity":"warning","source":"pipeline","line":2,"column":8,"end_column":13,"code":"warning.unused-source","message":"source product `notes` is never used as an input"}],"paths":[]}
exit 0
{"diagnostics":[{"severity":"error","source":"pipeline","line":5,"column":1,"end_column":7,"code":"path.path-problem","message":"output `copied` has no path rule; write `path copied: ...`, or a `default path:` for the pipeline or its stage"}]}
exit 1
```

An invocation `check` cannot carry out, such as options that cannot be combined, prints nothing on standard output and exits 1. It explains why on standard error, here `error: --json cannot be used with --path-rules`, which this example discards:

<!-- spit-doc: output
spit check copy.spit --json --path-rules 2>/dev/null || echo "exit $?"
--- copy.spit
source reading : Text .txt [site]
path reading: readings/{site}.txt
default path: {@product}/{site}
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(reading)
-->

```text
exit 1
```

- **`diagnostics`** is always present, empty when the file is clean. Each diagnostic has:
  - `severity`, `error` or `warning`;
  - `source`, `inventory` for a `.spitout` and `pipeline` for any other file checked;
  - `line`, `column` and `end_column`, counted from 1, with columns in UTF-16 code units and `end_column` one past the last character; all three are `null` when the problem has no place, as for a file that cannot be read (`cli.failed`);
  - `code`, such as `resolve.missing-input` or `warning.unused-source`, which stays the same when the message is reworded, so match on it;
  - `message`, for people; its words may change;
  - `file`, only when the problem is in a file other than the one checked, such as the pipeline a recipe names or a library it uses;
  - `related`, only when other places explain it, each with its `line`, `column`, `end_column`, `message` and, in another file, `file`.
- **`paths`** comes with a `.spit` that has no errors: each product whose path comes from a `default path:` line, with the `line` that makes it and the `path` it gets. With errors it is left out.
- **`calls`**, with `--calls`, takes the place of `paths`: see `--calls` above. With errors it is left out.
- **With `--hovers`**, a `.spit` always has `paths`, empty with errors, and adds `hovers`, `words` and `word_docs`; a recipe adds `hovers`, `words` and `word_docs`, and a `.spitout` adds `words` and `word_docs`. These are for spit-vscode, and their shape may change with it.
- Without `--hovers`, a recipe, a `.spitout` or a `.spitenv` gives `diagnostics` alone.

## A practical sequence

<!-- spit-doc: fragment -->

```sh
spit check dataset.spitin --path-rules
spit inputs dataset.spitin --unmatched
spit dag dataset.spitin --commands
spit artifacts dataset.spitin
spit dag dataset.spitin -o plan.spitdag
```

`artifacts` is most useful when expected jobs are missing or `dag` reports an incomplete input. `artifacts` reports incomplete artifacts and their causes.

### Symbol hovers

`check --json --hovers` keeps symbol signatures and details as plain text. Signatures may contain newlines: long operation signatures and multiple outputs are laid out at port boundaries, keeping nested generic types intact. Declaration details explain a generic body; call details show concrete products and types, and only the concrete expansion. Editors can render consecutive port mappings as one code block. The diagnostic and hover ranges are unchanged, using one-based UTF-16 columns with an exclusive end.
