# The `.spitdag` format

A `.spitdag` is what `spit dag -o` writes and `spit dag --json` prints: every job a pipeline resolves to over one dataset, each with its files and its commands. A backend that runs the jobs reads nothing else: no pipeline, path rule or command template. This page describes version 6, the version `src/spitdag` writes. See the [README](../README.md) for how a `.spitdag` is made, and `spit dag --commands` for a readable view of the same commands.

## Document

A `.spitdag` is one JSON object, followed by a newline:

```json
{
  "version": 6,
  "generator": {"name": "spit", "version": "0.2.2"},
  "root": "/data/study",
  "external_inputs": [ARTIFACT, ...],
  "targets": [ARTIFACT, ...],
  "executables": ["sort", ...],
  "removed": [REMOVAL, ...],
  "left_out": [LEFT_OUT, ...],
  "jobs": [JOB, ...]
}
```

| Field | Holds |
| --- | --- |
| `version` | The format's version, `6`. A change a reader must know about raises it. Version 6 added each job's [`checks`](#checks). |
| `generator` | The program that wrote the file, and its version. |
| `root` | The absolute dataset folder that every path is relative to, or `null` when it was not known: a `.spitout` that records no root. |
| `external_inputs` | Every artifact a job reads but no job writes, once each: the sources, and the outputs of stages left out. Ordered by path in natural order, the order `many` inputs take, so `wave2` comes before `wave10`. |
| `targets` | Every artifact a job writes but no job reads: what a full run leaves behind. Ordered by the job that writes it. |
| `executables` | The program each command, `verify` command and check starts with, once each, in text order. A command whose first word is a path names no program and is left out. A backend can check these are installed before running anything. |
| `removed` | What the input stage left out of the dataset, and why: see [Removal](#removal). `[]` when nothing was. |
| `left_out` | Outputs whose jobs could not be planned, each with its reasons: see [Left out](#left-out). `[]` for a complete plan. |
| `jobs` | Every job, each after the jobs it depends on. |

## Artifact

Every artifact is written the same way, wherever it appears:

```json
{"product": "cleaned", "entities": {"sub": "01", "ses": "1"}, "type": {"name": "Image", "args": []}, "path": "derivatives/cleaned/sub=01__ses=1.txt", "kind": "file"}
```

| Field | Holds |
| --- | --- |
| `product` | The product's name. An imported product keeps its prefix, as in `text::shard`. |
| `entities` | Each dimension and its value, as strings, in the product's declared order. A product with no dimensions has `{}`. |
| `type` | `null` for an untyped product; `{"name": N, "args": [TYPE, ...]}` for a named type, with its arguments; `{"variable": V}` for a type variable left unbound. |
| `path` | The artifact's file or folder, relative to `root`, with no `/` at its end. |
| `kind` | `"file"`, or `"folder"` for a product declared with a `/`, whose artifact is a folder of files: see [Folders](#folders). Version 5 added it; a reader written for version 4 does not know it. |

## Removal

Each artifact or group an `exclude` or `drop` rule removed, as the `.spitout` records it:

```json
{"product": "bold", "entities": {"run": "3", "ses": "02", "sub": "02"}, "rule": "exclude bold[sub=02,ses=02,run=3]", "origin": "line 4", "reason": "corrupted", "found": null}
```

| Field | Holds |
| --- | --- |
| `product` | The removed artifact's product, or `null` for a group, which removed every artifact whose identity includes `entities`. |
| `entities` | Each dimension and value, as strings, in name order. |
| `rule` | The rule that removed it, as written. |
| `origin` | Where the rule is: `line 4` of the recipe, or a line of a file an `exclude from` line names. `null` when not recorded. |
| `reason` | Why, from the rule's comment or a file's `reason` column; `null` when not given. |
| `found` | For a group a counting `drop` rule removed, how many it found; `null` otherwise. |

Nothing in `removed` is among the jobs' inputs: the record says what was left out, so a report can say so.

## Left out

Each output that could not be produced has its identity and the input gaps that prevented its job:

```json
{"identity": "report[store=s07]", "reasons": ["input `weeks` needs revenue[store=s07,week=2026-W36], which cannot be produced"]}
```

`dag --partial` fills this array while keeping every complete job. Plain `dag` fails if any output would be left out. The reasons are the same kinds shown by `spit artifacts`: a missing or ambiguous input, a collection below `@ min`, or an input artifact whose job cannot be completed. A `many` input in a partial plan uses only its complete members, so a downstream aggregate may still run.

## Job

```json
{
  "id": 4,
  "operation": "merge",
  "stage": ["preprocess", "combine"],
  "fingerprint": "357a0ff06e3e9e39",
  "inputs": {"items": [ARTIFACT, ...]},
  "outputs": {"output": ARTIFACT},
  "depends_on": [1, 2],
  "dependents": [6],
  "command": [ARGUMENT, ...],
  "verify": [[ARGUMENT, ...], ...],
  "checks": [CHECK, ...]
}
```

| Field | Holds |
| --- | --- |
| `id` | The job's number, from 1, unique in the file. |
| `operation` | The operation the job runs. |
| `stage` | The stage of the step that made the job, outermost first, such as `["preprocess", "combine"]`; `[]` outside every stage. |
| `fingerprint` | 16 hexadecimal digits identifying the job's work: see [Fingerprint](#fingerprint). |
| `inputs` | Each input port and the artifacts bound to it, in port order. A `one` port holds one artifact; a `many` port holds its collection in natural order. |
| `outputs` | Each output port and the artifact it writes. A single unnamed output is `output`. |
| `depends_on` | The jobs that write this job's inputs. |
| `dependents` | The jobs that read this job's outputs. |
| `command` | The command that writes the outputs, or `null` for an operation with none. |
| `verify` | The commands that check the inputs before `command` runs, in order; `[]` for none. |
| `checks` | The checks of single artifacts the job reads and writes, in the order they run: see [Checks](#checks). `[]` for none. |

### Commands

A command is a list of arguments, and each argument is a list of parts, joined with nothing between them. A part is either literal text, a JSON string, or an artifact's path, `{"path": P}` with `P` relative to `root`. The template `tool --in={raw} -o {@output}` becomes:

```json
[["tool"], ["--in=", {"path": "in/1.txt"}], ["-o"], [{"path": "out/1.txt"}]]
```

That is four arguments: `tool`, `--in=in/1.txt`, `-o` and `out/1.txt`. A `many` placeholder becomes one argument for each artifact in its collection.

`{image.dir}` and `{image.stem}` in a template become an output file's folder, `.` for the root itself, and its file name without its extension. Each is written with the file it is of, so a backend that moves files knows which one:

```json
[["-o"], [{"dir": "derivatives/image", "of": "derivatives/image/sub=01.nii.gz"}], ["-f"], [{"stem": "sub=01", "of": "derivatives/image/sub=01.nii.gz"}]]
```

Use the `dir` or `stem` text as the part, as a `path` part is used. Version 4 added these two parts; a reader written for version 3 does not know them.

Run each argument as one word, exactly as given: no shell is involved, so nothing in it is split, expanded or interpreted. Resolve each path against `root`, or run the command from `root`, where the relative paths name the right files.

### Running `verify`

A backend runs a job's `verify` commands, in order, before its `command`. If one fails, the job does not run, and neither does any job that depends on it, directly or through others.

## Checks

Each check runs one command on one artifact:

```json
{"when": "after", "check": "ndim(4)", "port": "output", "path": "derivatives/dwi/sub=01.mif", "command": [["check_ndim"], [{"path": "derivatives/dwi/sub=01.mif"}], ["4"]]}
```

| Field | Holds |
| --- | --- |
| `when` | `"before"` for a check of an input, which runs before the job's `verify` commands and its `command`; `"after"` for a check of an output, which runs after `command`, once the output exists. |
| `check` | The check as the pipeline attaches it, such as `ndim(4)` or `nonempty`, for reporting a failure. |
| `port` | The input port, for `"before"`, or the output port, for `"after"`, whose artifact it checks. |
| `path` | The checked artifact's path, relative to `root`. |
| `command` | The check's command, as a [command](#commands) is written. |

The `"before"` checks come first, then the `"after"` checks; within each, by port, then by artifact in the port's order, then in the order the pipeline attaches them. An input's checks include those of the source it reads. A check the job that writes an artifact runs on it is not repeated by a job in the same file that reads it.

### Running checks

A backend runs a job in this order: the `"before"` checks, the `verify` commands, `command`, the check that every output exists, then the `"after"` checks. If any fails, the job fails, even when `command` exited with status 0, and no job that depends on it runs. Report a failure with the check, the port and the path, as in `check ndim(4) failed on output derivatives/dwi/sub=01.mif`.

Checks are left out of the job's [fingerprint](#fingerprint), which identifies the job's work: a changed check does not make a job's outputs out of date. A backend that records successful jobs records the checks each passed with, such as the JSON of its `checks`. When a job is otherwise current but its checks differ from the record, the backend runs the checks alone on the existing files rather than rerunning `command`. If one fails, the job fails and its record is dropped, so the next run reruns it.

## Folders

A job that writes a folder owns it: SPIT rejects a pipeline that would put any other artifact inside it. So a backend may treat the folder as one output:

- Make its parent folder before the command runs, as for a file, but not the folder itself, which the tool makes. Some tools refuse to write into a folder that already exists.
- Remove a folder left by an earlier run before the command runs, so that files from that run do not mix with this one's.
- After the command, require the folder to exist. It may be empty.
- To tell whether a folder changed, look at the files under it, not the folder's own modification time, which changes only when an entry directly in it is added or removed.

A source folder may hold other sources, but never an output.

## Fingerprint

A job's fingerprint is a 64-bit FNV-1a hash of the compact JSON of what it reads, writes and runs: its `operation`, `inputs`, `outputs`, `command` and `verify`, each artifact with its product, entities, type, path and kind. Its `id`, `stage`, `depends_on`, `dependents` and `checks` are left out, so the fingerprint follows the work, not where the job falls in the plan.

The fingerprint changes when the job's command, its files, or which artifacts it reads or writes change. It does not read the files themselves, so it does not change when an input file's contents do; a backend that must rerun a job after its inputs change combines the fingerprint with its own record of the files, such as their modification times or hashes.
