# Command line

The commands validate source files, settle input inventories, and resolve concrete jobs. `spit help <command>` reports the installed binary’s options. File specifications in this manual define the accepted contents.

## Invocation

```text
spit check <pipeline.spit | recipe.spitin | inputs.spitout> [--path-rules] [--calls] [--json] [--stdin] [--hovers]
spit inputs <recipe.spitin> [--unmatched | --suggest | -o <file>]
spit inputs <pipeline.spit> --root <directory> [--unmatched | --suggest | -o <file>]
spit dag <recipe.spitin> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit dag <pipeline.spit> <inputs.spitout | -> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit dag <pipeline.spit> --root <directory> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit artifacts <recipe.spitin>
spit artifacts <pipeline.spit> <inputs.spitout | ->
spit artifacts <pipeline.spit> --root <directory>
```

Files come first; options follow them. `spit help` lists the commands, and `spit help <command>` or `spit <command> --help` gives one command's options.

| Command | Result |
| --- | --- |
| `check` | Compile a pipeline and report every problem the text shows, reading no data. Given a recipe, check its rules against the pipeline its `pipeline` line names. Given a `.spitout`, check the syntax of its records; `dag` and `artifacts` check them against a pipeline. |
| `inputs` | Scan the dataset folder with a recipe, apply its `exclude` rules, check its `require` rules, and print the `.spitout` with a record of what was removed. It writes nothing if a `require` rule fails. |
| `dag` | Resolve the jobs, and print each with its artifacts and dependencies. With `-o`, write them as a `.spitdag`. |
| `artifacts` | List every concrete artifact the inputs yield: the complete ones, then the incomplete ones with why each cannot be produced. Unlike `dag`, it does not stop at a missing, ambiguous, or too-small input or a coverage gap; see [artifact reports](#artifact-reports). |

| Option | Effect |
| --- | --- |
| `--root <directory>` | With a `.spit` pipeline given alone to `inputs`, `dag` or `artifacts`, the dataset folder to scan with the pipeline's own path rules, relative to where `spit` runs. A recipe or `.spitout` names its root with a `root` line instead, and `--root` with either is an error. |
| `-o <file>`, `--output <file>` | With `inputs`, write the `.spitout` to the file instead of standard output. With `dag`, write the `.spitdag`. |
| `--path-rules` | With `check`, list the path rule each product uses (its own, a stage's or the pipeline's default, the recipe's, or for an output the built-in `out/{@product}/{@entities}`), with any [extension](paths.md#extensions) added to it and where that is declared. |
| `--calls` | With `check` on a pipeline, list each call to an [operation carried out by steps](operations.md#operations-carried-out-by-steps) before the final `Pipeline valid.`, with the steps it expands to and no data read: the call as written, its line and stage, the operation's file and the first seven characters of that file's git blob id, then each step with the line of the library's body that writes it. A call in a body is shown under its caller, with its own steps beneath it. With `--json`, print `{"diagnostics":[...],"calls":[...]}` instead of the usual `check --json` output: one entry per call, in the order the pipeline's calls are made, with `id`, `parent` (the `id` of the call it is nested in, or `null`), `operation`, `outputs`, `inputs`, `line`, `stage`, `file`, `blob` and `steps`, each step with its `line`, `operation`, `outputs` and `inputs`; `calls` is left out when the pipeline has errors. It cannot combine with `--path-rules` or `--hovers`, and a recipe or `.spitout` has no calls to list. |
| `--unmatched` | With `inputs`, list files under the dataset root that match no source path rule, one per line, leaving out files at the pipeline's output paths, instead of writing a `.spitout`. |
| `--suggest` | With `inputs`, print `source` and `path` lines for the files under the dataset root that match no source path rule, instead of writing a `.spitout`; see [source suggestions](#source-suggestions). |
| `--paths` | With `dag`, print the file under every artifact. |
| `--counts` | With `dag`, print how many jobs each step resolves instead of the jobs: one row per step, as `cleaned = clean`, with its stage when the pipeline has stages, then the total. A step that resolves no jobs shows `0`, so an empty step or an unexpected expansion stands out before the plan is run. The steps a call to an [operation carried out by steps](operations.md#operations-carried-out-by-steps) makes are indented under the call, with the call's jobs in all. With `--commands` or `--paths`, print the counts before the jobs. With `-o`, print the counts and write the `.spitdag` too. It cannot be combined with `--json`, which prints the `.spitdag` itself. |
| `--commands` | With `dag`, print each job's checks, `verify` and command lines with their paths filled in, quoted as a shell reads them, so a line can be pasted into a shell run from the dataset folder. A job a call to an operation carried out by steps made starts with a `from:` line naming the call and the body's step. This is what plain `dag` prints. With `--paths`, print them under each job's artifacts. With `-o`, print the commands and write the `.spitdag` too, so the plan checked is the plan saved. |
| `--jobs` | With `dag`, list each job's operation, stage, and input and output artifacts with their types, in place of its commands. A pipeline whose operations have no `command` lines plans with this view; plain `dag` notes when no job has a command. |
| `--by-target` | With `artifacts`, group the incomplete artifacts by final target: each incomplete artifact no other incomplete job needs, with the incomplete artifacts it waits on nested under it, and each artifact and its reasons written once in the whole report. The complete artifacts are counted, not listed. |
| `--partial` | With `dag`, plan jobs whose inputs can be completed and record the artifacts left out of the `.spitdag`. A `many` input uses its complete members. Without it, `dag` stops at an incomplete job. |
| `--json` | With `dag`, print the `.spitdag`. With `check`, print diagnostics as JSON for editor use and stop, succeeding whatever they report. Each diagnostic has a `severity` of `error` or `warning`; those tied to a declaration, call, rule, command, or path include its `line`, and a `column` and `end_column` for the text it is about, such as one input of a call or one `{placeholder}`. Columns are 1-based and count UTF-16 code units, as editors do; `end_column` is one past the last character. When checking a recipe finds an error in its pipeline, the diagnostic includes `file` and positions in that pipeline. An error in a step a call to an [operation carried out by steps](operations.md#operations-carried-out-by-steps) makes is placed at the call, and its `related` list gives each place in the body it comes from, each with its `line`, `column`, `end_column`, a `message`, and the `file` when it is another file, such as a library. For a pipeline that checks clean, a `paths` list gives each product whose path no rule writes in full, with its `line` and its `path`, extension included, for the editor to show. |
| `--stdin` | With `check`, read the file's text from standard input, such as an editor's unsaved buffer. The file's path is still used to resolve `use` imports and a recipe's `pipeline` line. |
| `--hovers` | With `check --json`, include what an editor shows on hover: for a pipeline, compiler-backed operation and product explanations in a `hovers` array; for any file, SPIT's own words in `words` and `word_docs`. Reads no dataset. |

Pass `-` in place of the `.spitout` to read it from standard input.

Editor hovers include signatures, inferred product types and dimensions, call-local generic bindings, producer/consumer relationships, commands, stages, and effective path templates with their provenance. Each hover names its `kind` and `name`, gives a plain-text `signature` and `details` array, and uses the same 1-based UTF-16 `line`, `column`, and exclusive `end_column` convention as diagnostics. A call to an [operation carried out by steps](operations.md#operations-carried-out-by-steps) is described as written: its operation lists the body's steps and the steps this call expands to, each argument names the input it supplies, and a product the call makes names the call and the step that writes it. Broken lines are recovered so unrelated symbols remain available; steps that fail checking do not claim specialised types. A recipe or `.spitout` has no `hovers` array, since its names are its pipeline's. `--hovers` requires `--json`.

SPIT's own words, such as `source`, `@ vary(...)`, `where` in a `require` rule, `{@entities}`, `{image.stem}` or a `.spitout`'s `sources:`, are listed in `words`, each with its `line`, `column`, `end_column` and the `word` it is. `word_docs` gives each word used once, by its name: its `kind` (`keyword`, `selector`, `placeholder` or `header`), a SPIT `example`, a `summary` in plain text with code in backticks, and a `reference` link to its section of the [language reference](index.md). A word is told by where it is written, so a product called `each` is not the selector, and nothing in a comment is a word:

```json
"words": [{"line": 1, "column": 28, "end_column": 32, "word": "vary"}],
"word_docs": {"vary": {"kind": "selector", "example": "average = mean(processed @ vary(run))", "summary": "Collects a `many` input over the named dimensions, ...", "reference": "https://github.com/eclnz/spit/blob/main/docs/language-reference.md#operations-and-commands"}}
```


## Root selection

| Input form | Root | `--root` |
| --- | --- | --- |
| Pipeline alone | Required CLI directory, relative to the working directory | Required for scanning |
| Recipe | Required `root` line, relative to the recipe file, or absolute | Rejected |
| Pipeline and inventory | Optional inventory `root`, relative to the inventory file, or absolute | Rejected |

An inventory without a root permits logical planning without checking source files on disk. A recipe’s root cannot be omitted or replaced by a command-line root. With a known root, required source paths must exist with their declared file/folder kind. Output paths are validated before execution but need not exist yet.

Invalid: `spit dag dataset.spitin --root data`. The recipe already owns its root.


## Artifact reports

`artifacts` reports complete artifacts, incomplete artifacts and their causes, coverage gaps, and unused sources. It succeeds despite missing, ambiguous, or too-small inputs so that the report can be inspected; malformed files and invalid declarations still fail. A step with no driving artifacts has no candidate jobs, so no per-job gap is invented for it.

`dag --partial` preserves jobs with complete inputs and records omitted outputs in `left_out`. Collection minimums are applied after incomplete members are omitted. Plain `dag` stops at an incomplete job.

## Source suggestions

`inputs --suggest` groups unmatched files by shape and proposes source declarations and path rules. Suggestions are text for inspection, not changes to a pipeline. Previously generated output paths and files inside known output folders are excluded from the unmatched-source scan. `inputs --unmatched` lists unread paths instead. Both modes replace the ordinary inventory output.

## Diagnostics

Text-mode errors prevent a command from producing its result. Warnings do not. `check --json` reports language errors in its JSON while succeeding; consumers must inspect diagnostic severity rather than infer validity from that exit status. Invocation and file-read failures still fail. A missing recipe root directory is a warning in `check` and an error when scanning needs it. Missing operation commands are warnings during planning. Exact text diagnostic wording is not a syntax contract.
