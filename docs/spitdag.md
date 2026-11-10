# The `.spitdag` format

A `.spitdag` is what `spit dag -o` writes and `spit dag --json` prints: every job a pipeline resolves to over one dataset, each with its files and its commands. A backend that runs the jobs reads nothing else: no pipeline, path rule or command template. This page describes version 15, the version SPIT writes. Use `spit dag --commands` for a readable view of the same commands.

## Document

A `.spitdag` is one JSON object, followed by a newline:

```json
{
  "version": 15,
  "generator": {"name": "spit", "version": "0.5.0"},
  "root": "/data/study",
  "output_root": "/data/study-results",
  "external_inputs": [ARTIFACT, ...],
  "targets": [ARTIFACT, ...],
  "executables": ["sort", ...],
  "tools": [TOOL, ...],
  "removed": [REMOVAL, ...],
  "left_out": [LEFT_OUT, ...],
  "reviews": [REVIEW, ...],
  "pipeline_folder": "/home/lab/study",
  "pipeline_files": [FILE, ...],
  "calls": [CALL, ...],
  "jobs": [JOB, ...]
}
```

| Field | Holds |
| --- | --- |
| `version` | The format's version, `15`. A change a reader must know about raises it. Version 6 added each job's [`checks`](#checks). Version 7 added `pipeline_files`, `calls` and each job's `origin`: see [Where jobs come from](#where-jobs-come-from). Version 8 gives each job its [`commands`](#commands) in place of version 7's `command`, each with its `stdin` and `stdout`, see [Standard input and output](#standard-input-and-output), and added its `cores` and `memory`, see [Cores and memory](#cores-and-memory). Version 9 gives every job an `origin`, with the `file` and `line` of its step, where version 8 left it `null` for a step written in the pipeline. Version 10 added `pipeline_folder` and the `here` command part. Version 11 removed each job's `fingerprint`: see [Identifying work](#identifying-work). Version 12 added `output_root` and each artifact's `under`: see [Roots](#roots). Version 13 added the [`scalar` part](#numbers-read-from-files). Version 14 added [`reviews`](#reviews). Version 15 added `tools` and each job's `tools`, see [Tools](#tools), each job's `anchored`, the [`join` and `list` parts](#collections), and `under` `"pipeline_folder"`: see [Roots](#roots). |
| `generator` | The program that wrote the file, and its version. |
| `root` | The absolute dataset folder, which the sources are under, or `null` when it was not known: a `.spitout` that records no root. |
| `output_root` | The absolute folder the files jobs write are under, and the folder commands run from: see [Roots](#roots). It is `root` when the pipeline names no output root, and `null` when neither is known. |
| `external_inputs` | Every artifact a job reads but no job writes, once each: the sources. Ordered by path in natural order, the order `many` inputs take, so `wave2` comes before `wave10`. |
| `targets` | Every artifact a job writes but no job reads: what a full run leaves behind. Ordered by the job that writes it. |
| `executables` | The program each command, `verify` command, check and tool probe starts with, once each, in text order. A command whose first word is a path names no program and is left out. A backend can check these are installed before running anything. |
| `tools` | What each tool is on the machine the jobs run on, from the recipe's `.spitenv`: see [Tools](#tools). `[]` without one. |
| `removed` | What the input stage left out of the dataset, and why: see [Removal](#removal). `[]` when nothing was. |
| `left_out` | Outputs whose jobs could not be planned, each with its reasons: see [Left out](#left-out). `[]` for a complete plan. |
| `reviews` | Each review the pipeline attaches, with its file and what it withheld: see [Reviews](#reviews). `[]` for a pipeline with none. |
| `pipeline_folder` | The absolute path of the folder holding the pipeline's own file, which a [`here` part](#commands) is under; `null` when SPIT did not read the pipeline from a file. |
| `pipeline_files` | Every file the pipeline was read from, with its git blob id: see [Where jobs come from](#where-jobs-come-from). |
| `calls` | Every call to an operation carried out by steps: see [Where jobs come from](#where-jobs-come-from). `[]` for a pipeline that makes none. |
| `jobs` | Every job, each after the jobs it depends on. |

## Artifact

Every artifact is written the same way, wherever it appears:

```json
{"product": "cleaned", "entities": {"sub": "01", "ses": "1"}, "type": {"name": "Image", "args": []}, "path": "derivatives/cleaned/sub=01__ses=1.txt", "under": "output_root", "kind": "file"}
```

| Field | Holds |
| --- | --- |
| `product` | The product's name. An imported product keeps its prefix, as in `text::shard`. |
| `entities` | Each dimension and its value, as strings, in the product's declared order. A product with no dimensions has `{}`. |
| `type` | `null` for an untyped product; `{"name": N, "args": [TYPE, ...]}` for a named type, with its arguments; `{"variable": V}` for a type variable left unbound. |
| `path` | The artifact's file or folder, relative to the folder `under` names, with no `/` at its end. No two artifacts share a path, even under different roots. |
| `under` | `"root"`, `"output_root"` or `"pipeline_folder"`: the top-level field holding the folder `path` is relative to. |
| `kind` | `"file"`, or `"folder"` for a product declared with a `/`, whose artifact is a folder of files: see [Folders](#folders). Version 5 added it; a reader written for version 4 does not know it. |

## Roots

A `.spitdag` has two folders, which are the same unless the pipeline or recipe names an [output root](language-reference.md#the-output-root):

- `root`, the dataset's folder: the sources are under it, and so is every file a job writes [beside a source](language-reference.md#files-a-tool-writes-beside-another), whose `under` is `"root"`.
- `output_root`: every other file a job writes is under it, with `under` `"output_root"`.

A [`here` source](language-reference.md#files-beside-the-pipeline), a file kept beside the pipeline such as a script, is under a third folder, `pipeline_folder`, with `under` `"pipeline_folder"`. It is an input like any other source, so a backend that reruns a job when its inputs change reruns it when the file does. No job writes under `pipeline_folder`, and a backend refuses a DAG whose job does.

A path is unique across all three, so a command part, a `stdin` or `stdout` and a check name an artifact by its `path` alone; look up that artifact's `under` to find its folder. Make `output_root` before running the first job, and keep a backend's own state under it rather than under `root`, which may be read-only.

## Removal

Each artifact or group a named or conditional `exclude` rule removed, as the `.spitout` records it:

```json
{"product": "bold", "entities": {"run": "3", "ses": "02", "sub": "02"}, "rule": "exclude bold[sub=02,ses=02,run=3]", "origin": "line 4", "reason": "corrupted", "found": null}
```

| Field | Holds |
| --- | --- |
| `product` | The removed artifact's product, or `null` for a group, which removed every artifact whose identity includes `entities`. |
| `entities` | Each dimension and value, as strings, in name order. |
| `rule` | The rule that removed it, as written. |
| `origin` | Where the rule is: `line 4` of the recipe. `null` when not recorded. |
| `reason` | Why, from a named exclusion's comment; `null` when not given. |
| `found` | For a group a counting conditional `exclude` rule removed, how many it found; `null` otherwise. |

Nothing in `removed` is among the jobs' inputs: the record says what was left out, so a report can say so.

A [review](language-reference.md#reviews) leaves no record here: what it withheld is under [Reviews](#reviews).

## Left out

Each output that could not be produced has its identity and the input gaps that prevented its job:

```json
{"identity": "report[store=s07]", "reasons": ["input `weeks` needs revenue[store=s07,week=2026-W36], which cannot be produced"]}
```

`dag --partial` fills this array while keeping every complete job. Plain `dag` fails if any output would be left out. A reason about a step that a call to an [operation carried out by steps](language-reference.md#operations-carried-out-by-steps) made starts with the call, as ``in `m, t = L::summarise(...)`: ``. The reasons are the same kinds shown by `spit artifacts`: a missing or ambiguous input, a collection below `@min`, or an input artifact whose job cannot be completed. A `many` input in a partial plan uses only its complete members, so a downstream aggregate may still run.

## Reviews

SPIT applies a [review](language-reference.md#reviews)'s verdicts when it makes the plan: an artifact a review rejects, or that no row judges yet, is in no job's inputs that the review covers, and the jobs that would have read it are not in the DAG. `reviews` records each review the pipeline attaches to a product, in declaration order, so a backend can report what is waiting for a verdict and where a person records it:

```json
{
  "name": "looked_at",
  "file": "/home/lab/study/qc/looked_at.csv",
  "found": true,
  "verdicts": 3,
  "unmatched": [4],
  "targets": [
    {"artifact": ARTIFACT, "port": null, "verdict": "fail", "reason": "ghosting", "line": 3, "withholds": ["fits[sub=02]", "reports[sub=02]"], "shrinks": ["total"]},
    {"artifact": ARTIFACT, "port": null, "verdict": null, "reason": null, "line": null, "withholds": ["fits[sub=03]", "reports[sub=03]"], "shrinks": ["total"]}
  ]
}
```

| Field | Holds |
| --- | --- |
| `name` | The review, as `review` declares it. |
| `file` | The absolute path of the file the recipe's `review ... from` line names, or `null` when it names none. |
| `found` | Whether `file` existed when SPIT made the plan. A file with a header and no rows is `found` with `verdicts` `0`. |
| `verdicts` | How many rows the file holds. |
| `unmatched` | The lines of rows that match no artifact the review judges. |
| `targets` | Each artifact the review withheld, in the order SPIT met them. A passed artifact is not listed: it is planned as usual. |

Each target:

| Field | Holds |
| --- | --- |
| `artifact` | The artifact a person inspects, with its path: a source, which is under `root` even when no job reads it, or an output of a job in the DAG. |
| `port` | `{"operation": O, "port": P}` when the review is on an input port and withholds the artifact from that port alone; `null` for a review on a source or an output, which withholds it from every reader. |
| `verdict` | `"fail"` for a rejection, or `null` when no row judges it yet. |
| `reason`, `line` | The rejecting row's reason, or `null` when it gives none, and its line in `file`; both `null` when no row judges it. |
| `withholds` | The outputs not planned because of it, directly or through outputs withheld in turn, by identity as `left_out` writes them. |
| `shrinks` | The outputs of jobs in the DAG that take a `many` input without it. |

A withheld artifact is not incomplete: a plan with reviews is complete, and `left_out` holds only what could not be planned. The record is a snapshot of the verdicts when the plan was made. Changing the review file changes nothing in a saved `.spitdag`; make the plan again to apply the new verdicts.

## Job

```json
{
  "id": 4,
  "operation": "merge",
  "stage": ["preprocess", "combine"],
  "origin": {"call": null, "file": 0, "line": 7},
  "inputs": {"items": [ARTIFACT, ...]},
  "outputs": {"output": ARTIFACT},
  "depends_on": [1, 2],
  "dependents": [6],
  "commands": [COMMAND, ...],
  "verify": [[ARGUMENT, ...], ...],
  "checks": [CHECK, ...],
  "cores": 8,
  "memory": 17179869184,
  "tools": ["mrtrix3", "fsl"],
  "anchored": false
}
```

| Field | Holds |
| --- | --- |
| `id` | The job's number, from 1, unique in the file. |
| `operation` | The operation the job runs. |
| `stage` | The stage of the step that made the job, outermost first, such as `["preprocess", "combine"]`; `[]` outside every stage. |
| `origin` | Where the step that made the job is written, as `{"call": 0, "file": 1, "line": 14}`. See [Where jobs come from](#where-jobs-come-from). |
| `inputs` | Each input port and the artifacts bound to it, in port order. A `one` port holds one artifact; a `many` port holds its collection in natural order. |
| `outputs` | Each output port and the artifact it writes. A single unnamed output is `output`. |
| `seed_folders` | When present, a map from each seeded output folder port to the single input folder port that supplies its initial contents. |
| `depends_on` | The jobs that write this job's inputs. |
| `dependents` | The jobs that read this job's outputs. |
| `commands` | The commands that write the outputs, run in order until one fails: see [Commands](#commands). `[]` for an operation with none. |
| `verify` | The commands that check the inputs before `commands` run, in order; `[]` for none. Each is a list of arguments. |
| `checks` | The checks of single artifacts the job reads and writes, in the order they run: see [Checks](#checks). `[]` for none. |
| `cores` | The number of cores the job uses, a whole number from 1: see [Cores and memory](#cores-and-memory). |
| `memory` | The most memory the job needs, in bytes, or `null` for no claim. |
| `tools` | The names of the [tools](#tools) the job depends on, in the order of the top-level `tools`. |
| `anchored` | `true` when the job's tools write its file paths into its outputs, from an [`anchored`](language-reference.md#tools-that-store-their-paths) operation: see [Identifying work](#identifying-work). |

### Commands

Each of a job's `commands` is an object:

```json
{"command": [["samtools"], ["index"], [{"path": "out/bam/sample=A.bam"}]], "stdin": null, "stdout": null}
```

| Field | Holds |
| --- | --- |
| `command` | The program and its arguments, a list of arguments as below. |
| `stdin` | The path of the input file the command reads on its standard input, or `null`: see [Standard input and output](#standard-input-and-output). |
| `stdout` | The path of the output file the command's standard output is written to, or `null`. |

Most jobs have one command. A [command of several lines](language-reference.md#commands-of-several-lines) gives a job one for each line, in the order written. Run them in that order, each once the one before it exits with status 0. When one fails, the job fails, and the commands after it do not run; report which, by its place from 1, as in `command 2 exited with status 1`.

A command's `command`, as a `verify` command and a check's, is a list of arguments, and each argument is a list of parts, joined with nothing between them. A part is either literal text, a JSON string, or an artifact's path, `{"path": P}` with `P` the artifact's `path`. The template `tool --in={raw} -o {@output}` becomes:

```json
[["tool"], ["--in=", {"path": "in/1.txt"}], ["-o"], [{"path": "out/1.txt"}]]
```

That is four arguments: `tool`, `--in=in/1.txt`, `-o` and `out/1.txt`. A `many` placeholder becomes one argument for each artifact in its collection.

`{image.dir}` and `{image.stem}` in a template become an output file's folder, `.` for its root itself, and its file name without its extension. Each is written with the file it is of, so a backend that moves files knows which one:

```json
[["-o"], [{"dir": "derivatives/image", "of": "derivatives/image/sub=01.nii.gz"}], ["-f"], [{"stem": "sub=01", "of": "derivatives/image/sub=01.nii.gz"}]]
```

Use the `dir` or `stem` text as the part, as a `path` part is used. Version 4 added these two parts; a reader written for version 3 does not know them.

`{@here}` in a template, the folder of the file that declares the operation, becomes a `here` part holding that folder relative to `pipeline_folder`, `""` for `pipeline_folder` itself:

```json
[["python3"], [{"here": "libs"}, "/scripts/table.py"]]
```

Use `pipeline_folder`, then `/` and the part's text unless it is empty, as the part's text. A part is written relative so that the commands do not change when the pipeline moves. Version 10 added this part; a reader written for version 9 does not know it.

### Numbers read from files

`@scalar(tr, min=5)` in a template, the number in an input's file, becomes a `scalar` part holding the file's path and the bounds, with `null` for one not given:

```json
[["oxford_asl"], ["--tr"], [{"scalar": "derivatives/tr/sub=01.txt", "min": 5, "max": null}]]
```

The path is that of one of the job's inputs, a file. Read the file once each time the job runs, after its `"before"` [checks](#checks) and before its first `verify` command, and use what it holds for every `scalar` part naming it, in `verify` and in `commands`:

- Remove the whitespace around the file's text. What is left must be one number written as a decimal: an optional `+` or `-`, digits with at most one `.` among or around them, and an optional exponent, `e` or `E` and a whole number, such as `6.234000`, `.5` or `1e-3`. It must be finite as a double.
- The number must be at least `min` and at most `max`, each a JSON number.
- The part's text is the trimmed text, as the file holds it.

When the file is missing, holds anything else, or is out of bounds, the job fails before its `verify` commands, as a failed `verify` command fails it. Report the path and what the file holds, as in `derivatives/tr/sub=01.txt holds 4.1, which is under the min of 5`.

The part names an input, so a backend that reruns a job when an input changes reruns it when the number does. Version 13 added this part; a reader written for version 12 does not know it.

Run each argument as one word, exactly as given: no shell is involved, so nothing in it is split, expanded or interpreted. Run the command from `output_root`, and resolve each path against its artifact's root: a path under `output_root` is then right as written, and one under `root` names the right file once joined to `root`. `spit dag --commands` shows such a path reached from `output_root`, as `../data/in.txt`.

### Collections

`@join(images)` and `@list(images)` in a template, a `many` input's paths as one argument, become a `join` part, with its separator, and a `list` part. Each holds the paths of the input's artifacts, in the order the `many` placeholder gives them:

```json
[["--inputs=", {"join": ["in/run=2.nii.gz", "in/run=10.nii.gz"], "separator": ","}], ["--file-list"], [{"list": ["in/run=2.nii.gz", "in/run=10.nii.gz"]}]]
```

Each path names one of the job's inputs. A `join` part's text is each path, resolved against its artifact's root as a `path` part's is, joined with the separator; a path holding a nonempty separator cannot be split apart again, so refuse the job. A `list` part's text is the path of a file that holds each path, made absolute, followed by a newline, in order; an empty list is an empty file. Write the file when the job starts, before its `verify` commands, give the same file to every command and `verify` command of the job that names the same paths, and remove it when the job ends, whether it succeeds, fails or is cancelled. Neither part is a check's, a program or a redirection. The file's own path is not part of the work: see [Identifying work](#identifying-work). Version 15 added these parts.

`@join(images.run)`, of a dimension's values, is literal text in the DAG.

### Standard input and output

A command written `wc -l < {table} > {@output}` reads an input on its standard input and writes an output on its standard output. It names the two files in `stdin` and `stdout`, and its `command` holds only the program and its arguments:

```json
"commands": [{"command": [["wc"], ["-l"]], "stdin": "in/sheet=a.tsv", "stdout": "out/lines/sheet=a.txt"}]
```

`stdout` is always the path of one of the job's `outputs`, a file, and never appears in that command's `command`. At most one of a job's commands has a `stdout`, and only the commands after it name its path. `stdin` is the path of one of the job's `inputs`, a file. Either is `null` when the command does not redirect it. `verify` commands and checks never redirect either.

- Open `stdin` for reading and connect it to the command's standard input. When it is `null`, the command's standard input is whatever the backend gives every command.
- Make `stdout`'s folder, as for any output, then connect a file for writing to the command's standard output. Write it to a temporary file in the same folder, and rename that onto `stdout` only once the command exits with status 0, before the next command runs, so a failed job never leaves a partial output at the path. When `stdout` is `null`, the command's standard output is the backend's, which typically logs it.
- The command's standard error is the backend's in either case.

The output is otherwise like any other: it must exist once the job's commands succeed, as [Missing outputs](#missing-outputs) says, which an empty file does, and its `"after"` checks run on it.

### Cores and memory

`cores` and `memory` say what a job needs while its commands run, from its operation's `resources` line, so a backend that runs jobs at once can keep from oversubscribing the machine:

```json
"commands": [{"command": [["bwa"], ["mem"], ["-t"], ["8"], [{"path": "reference/genome.fa"}], [{"path": "in/sample=01.fastq.gz"}]], "stdin": null, "stdout": null}],
"cores": 8,
"memory": 17179869184
```

- `cores` is always a whole number of at least 1. A job whose operation declares none has `1`.
- `memory` is a whole number of bytes, at most 2^53, or `null` when the operation claims none.
- Start a job only when the cores and memory it needs are free beside those of the jobs already running, and count them as taken until its `"after"` checks finish. A job that needs more than the backend has in all cannot be started that way: refuse it, or run it alone, as the backend chooses.
- `memory` is the most each of the job's `verify` commands and `commands` may hold, together with every process it starts. A backend may stop a job that holds more, and report it as failed. Treat `null` as needing none and setting no limit.
- A command holds the number of cores where the pipeline asked for it, with `{@cores}`, as `["-t"], ["8"]` above, so the tool uses what it was given; a backend changes nothing in `commands`.

`cores` and `memory` say how to schedule the job, not what it makes, so a backend that identifies a job's work leaves them out.

### Running `verify`

A backend runs a job's `verify` commands, in order, before its `commands`. If one fails, the job does not run, and neither does any job that depends on it, directly or through others.

### Missing outputs

Every artifact in a job's `outputs` must exist once its last command exits with status 0: a file, or for a `"folder"`, a folder, which may be empty. A job that leaves one missing fails, as a failed command does: a backend does not record it as a success, does not run its `"after"` checks, and runs no job that depends on it. Report each missing output with its port and path, as in `did not create output meta derivatives/image/sub=01.json`.

A declared output is never optional. A tool may skip a file because of how it is run, as `dcm2niix -b n` writes no `.json` sidecar; the job then fails, and the fix is an operation that does not declare that output.

## Checks

Each check runs one command on one artifact:

```json
{"when": "after", "check": "ndim(4)", "port": "output", "path": "derivatives/dwi/sub=01.mif", "command": [["check_ndim"], [{"path": "derivatives/dwi/sub=01.mif"}], ["4"]]}
```

| Field | Holds |
| --- | --- |
| `when` | `"before"` for a check of an input, which runs before the job's `verify` commands and its `commands`; `"after"` for a check of an output, which runs after `commands`, once the output exists. |
| `check` | The check as the pipeline attaches it, such as `ndim(4)` or `nonempty`, for reporting a failure. |
| `port` | The input port, for `"before"`, or the output port, for `"after"`, whose artifact it checks. |
| `path` | The checked artifact's path. |
| `command` | The check's program and arguments, a list of arguments as a [command's](#commands) `command` is. |

The `"before"` checks come first, then the `"after"` checks; within each, by port, then by artifact in the port's order, then in the order the pipeline attaches them. An input's checks include those of the source it reads. A check the job that writes an artifact runs on it is not repeated by a job in the same file that reads it.

### Running checks

A backend runs a job in this order: the `"before"` checks, the reads of its [`scalar` parts](#numbers-read-from-files), the `verify` commands, `commands`, the check that every output exists, then the `"after"` checks. If any fails, the job fails, even when its commands exited with status 0, and no job that depends on it runs. Report a failure with the check, the port and the path, as in `check ndim(4) failed on output derivatives/dwi/sub=01.mif`.

A backend that identifies a job's work leaves its checks out: a changed check does not make a job's outputs out of date. A backend that records successful jobs records the checks each passed with, such as the JSON of its `checks`. When a job is otherwise current but its checks differ from the record, the backend runs the checks alone on the existing files rather than rerunning `commands`. If one fails, the job fails and its record is dropped, so the next run reruns it.

## Folders

A job that writes a folder owns it: SPIT rejects a pipeline that would put any other artifact inside it. So a backend may treat the folder as one output:

- For an output in `seed_folders`, copy its input folder's contents into the owned output after input checks and `verify`, before the first command. Each attempt starts with the input's contents. A relative link may be preserved if its target stays inside the tree. The copy must isolate all writes from the input.
- For an ordinary output folder, before the job's first command runs, remove what an earlier run left at the folder's path, so that its files do not mix with this run's, then make the folder, empty, with its parents. The tool writes into a folder that exists, as a tool given a folder to fill, such as `bwa index -p` or `dcm2niix -o`, expects. A tool that refuses to write into an existing folder is wrapped in a command that removes it first.
- After the last command, require the folder to exist. It may be empty.
- To tell whether a folder changed, look at the files under it, not the folder's own modification time, which changes only when an entry directly in it is added or removed.

A source folder may hold other sources, but never an output.

## Where jobs come from

An operation can be carried out by a body of steps rather than a command, and a library can declare it (see [Operations carried out by steps](language-reference.md#operations-carried-out-by-steps)). A call to one becomes the body's steps, so its jobs are ordinary jobs. Three fields say where they came from.

`pipeline_files` lists each file the pipeline was read from, once:

```json
"pipeline_files": [
  {"path": "act.spit", "blob": "9f2c0e5d1c4b7a3e8f6d2b1a0c9e8d7f6a5b4c3d"},
  {"path": "libs/mrtrix_dwi.spit", "blob": "3b18e5a7c6d4f2e1b0a9c8d7e6f5a4b3c2d1e0f9"}
]
```

The first is the pipeline itself, by its file name. The others are the files it imports, at any depth, each by its path from the pipeline's folder. `blob` is the file's git blob id, the SHA-1 of `blob <length>\0` and its bytes, which is what `git hash-object <file>` prints. `git log --all --find-object=<blob>` finds the revisions that hold that exact text, and `git hash-object` on a checkout's files tells whether the plan came from them.

`calls` lists each call to an operation carried out by steps. Calls written in the pipeline come in the order they are written, each followed by the calls its body makes, in the order the body writes them, so a call's `parent` always comes before it:

```json
"calls": [
  {"operation": "mrx::clean", "instance": "dwi", "parent": null, "file": 1, "at": {"file": 0, "line": 12}},
  {"operation": "mrx::denoise", "instance": "dwi::denoised", "parent": 0, "file": 1, "at": {"file": 1, "line": 31}}
]
```

| Field | Holds |
| --- | --- |
| `operation` | The operation called, with its import prefix. |
| `instance` | The name the call's own products are filed under: the call's first output. A call in a body is named under its caller's instance, as in `dwi::denoised`. |
| `parent` | The call whose body holds this call, by its position in `calls`; `null` for a call written in the pipeline. |
| `file` | The file that declares the operation, by its position in `pipeline_files`. |
| `at` | Where the call is written: a file, by its position in `pipeline_files`, and a line from 1. |

A job's `origin` says where the step that made it is written:

- A step written in the pipeline has `"call": null`, and its `file`, by position in `pipeline_files`, and `line` are where the step is written.
- A step in the body of an operation a call expanded has `call`, the call's position in `calls`, and its `file` is the call's `file`, the file that declares the operation. Following `parent` from the call gives every call the job is nested in.

`file` is `null` when SPIT read the pipeline from no file, and then `pipeline_files` is empty.

None of these is part of a job's work. An edit elsewhere in a library, such as to a comment, changes a blob id and changes no command.

## Tools

A recipe names a `.spitenv`, which says what each tool is on the machine the jobs run on, and the pipeline says which tools each operation's jobs depend on: see [Tools](language-reference.md#tools). Each tool is an object:

```json
{"name": "mrtrix3", "identity": null, "probe": ["mrconvert", "-version"]}
{"name": "synthseg", "identity": "docker.io/lab/synthseg@sha256:4f1c", "probe": null}
```

| Field | Holds |
| --- | --- |
| `name` | The tool's name, unique in `tools`. |
| `identity` | The text the `.spitenv` gives, compared as it is; `null` for a tool with a probe. |
| `probe` | A command as a list of plain strings, run with no shell; `null` for a tool with an `identity`. |

- Run each probe once, before any job, on the machine the jobs run on, from the dataset root. Its standard output, as bytes, is the tool's identity. A probe that fails, or cannot be started, stops the run before any job starts.
- A job's `tools` names each tool it depends on. A job with `[]` depends on none, as every job does in a DAG without a `.spitenv`.
- A tool changes which jobs must run again, never how a job runs.

## Identifying work

SPIT writes no fingerprint: a backend that decides whether a job must run again identifies the work itself. The document holds what it needs.

- A job's `inputs` and `outputs` give each artifact's `product` and `entities`, which identify it apart from its `path`.
- A command part `{"path": P}`, `{"dir": D, "of": P}`, `{"stem": S, "of": P}` or `{"scalar": P, ...}` names the job's input or output whose `path` is `P`, and a `join` or `list` part names an input by each of its paths. Each path belongs to one artifact, so replacing `P` with that artifact's product and entities gives the command without its storage paths. The same holds for `stdin`, `stdout` and a check's `path`.
- A literal part, a `here` part, a `join` part's separator and the order of arguments, commands and a `many` port's artifacts are part of the work as written. Where a `list` part's file is written is not.
- For a job whose `anchored` is `true`, the `path` of each of its inputs and outputs, and the absolute `root` and `output_root`, are part of its work too: its outputs hold those paths, so a change of path or of root makes it stale, and a backend reruns it rather than moving its outputs to new paths.
- A job's `tools`, each with its identity in the top-level `tools`, are part of its work: a tool whose identity changed changes the work of every job that names it.
- An artifact's `type`, a job's `id`, `stage`, `origin`, `cores`, `memory` and `checks` say where the job came from or how to run it, not what it makes.

A backend that must rerun a job after its inputs change combines its identity of the work with its own record of the files, such as their modification times or hashes.
