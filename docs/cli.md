# Command line reference

SPIT checks a pipeline, settles a dataset's inputs, and resolves jobs. Run `spit help` or `spit help <command>` for the binary's help. The [language manual](language-reference.md) defines the accepted file formats.

| Command | Purpose | Common invocation |
| --- | --- | --- |
| `check` | Validate a pipeline, recipe, or input inventory | `spit check pipeline.spit --path-rules` |
| `inputs` | Scan sources and settle recipe rules | `spit inputs dataset.spitin -o dataset.spitout` |
| `dag` | Resolve and inspect concrete jobs | `spit dag dataset.spitin --counts --commands` |
| `artifacts` | Explain complete and incomplete artifacts | `spit artifacts dataset.spitin` |

## Input forms

`check` accepts one `.spit`, `.spitin`, or `.spitout` file. `inputs` accepts a `.spitin` recipe, or a `.spit` pipeline with an inline `root` or `--root <directory>`. `dag` and `artifacts` accept those same forms, or a pipeline with a `.spitout`. A recipe names its pipeline and either declares a root or inherits that pipeline's root. Inline roots are relative to their declaring file; `--root` is relative to the working folder and is an error when the pipeline already declares a root. A `.spitout` does not name its pipeline, so pass the pipeline separately.

```sh
spit dag dataset.spitin
spit dag pipeline.spit dataset.spitout
spit dag pipeline.spit --root data
```

`spit dag pipeline.spit --tree` reads the pipeline and its imports only. A compact overview names the products connecting stages. Within each stage, lines connect product producers to their consumers; matching product names link stage boundaries. Each distinct input has its own arrow into a boxed multi-input operation; simple chains use parentheses. Reused products branch along one live line, and `╪` marks a crossing without a join. Connected diagrams pack operations across a 120-column target and fold back through continuous product lines. Very wide labels or live product frontiers use the compact vertical layout. Output branches appear after operations. Calls to operations with bodies remain single nodes, followed by one diagram per used body, including nested bodies. Unused sources are listed. This view accepts no dataset or other flags and leaves resolved-job output unchanged.

Use `-` in place of a `.spitout` to read it from standard input. A recipe and a `.spitout` can be used to inspect an existing dataset without writing a DAG file.

## Inspection and output flags

| Flag | With | Effect |
| --- | --- | --- |
| `--path-rules` | `check` | List each product's effective path rule and its source. |
| `--calls` | `check` on a `.spit` | List calls to operations carried out by steps and their expanded steps; with `--json`, include a `calls` array. |
| `--unmatched` | `inputs` | List dataset files no source path rule reads. |
| `--tree` (`--ascii`) | `dag` on one `.spit` | Show product/step topology without reading dataset inputs. |
| `--counts` | `dag` | Print a job count for every step and a total. |
| `--paths` | `dag` | Print artifact paths in the plan. |
| `--commands` | `dag` | Print the commands the runner would execute. |
| `--partial` | `dag` | Keep jobs with complete inputs and record left-out outputs. |
| `--by-target` | `artifacts` | Group incomplete artifacts under the final targets they prevent. |
| `-o <file>` | `inputs`, `dag` | Save a `.spitout` or `.spitdag`, respectively. |
| `--json` | `check`, `dag` | Print structured diagnostics or the DAG. |
| `--stdin` | `check` | Read the file text from standard input, retaining its path for relative references. |
| `--hovers` | `check --json` | Include editor hovers and documentation for language words. |

`--counts` can precede either job view. `--paths` and `--commands` can print together. With `-o`, `--counts` and `--commands` also print while the DAG is saved; `--paths` conflicts with `-o`. `--json` is a separate output form. `check --calls` cannot combine with `--path-rules` or `--hovers`. For diagnostics fields, consult `spit help check`.

`check --json` writes diagnostics to standard output and exits 1 if any are errors. Warnings alone exit 0.

## A practical sequence

```sh
spit check dataset.spitin --path-rules
spit inputs dataset.spitin --unmatched
spit dag dataset.spitin --counts --commands
spit artifacts dataset.spitin
spit dag dataset.spitin -o plan.spitdag
```

`artifacts` is most useful when expected jobs are missing or `dag` reports an incomplete input. `artifacts` reports incomplete artifacts and their causes.

### Symbol hovers

`check --json --hovers` keeps symbol signatures and details as plain text. Signatures may contain newlines: long operation signatures and multiple outputs are laid out at port boundaries, keeping nested generic types intact. Declaration details explain a generic body; call details show concrete products and types, and only the concrete expansion. Editors can render consecutive port mappings as one code block. The diagnostic and hover ranges are unchanged, using one-based UTF-16 columns with an exclusive end.
