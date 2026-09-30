# The `.spitdag` format

A `.spitdag` is what `spit dag -o` writes and `spit dag --json` prints: every job a pipeline resolves to over one dataset, each with its files and its commands. A backend that runs the jobs reads nothing else: no pipeline, path rule or command template. This page describes version 3, the version `src/spitdag.rs` writes. See the [README](../README.md) for how a `.spitdag` is made, and `spit dag --commands` for a readable view of the same commands.

## Document

A `.spitdag` is one JSON object, followed by a newline:

```json
{
  "version": 3,
  "generator": {"name": "spit", "version": "0.2.0"},
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
| `version` | The format's version, `3`. A change a reader must know about raises it. |
| `generator` | The program that wrote the file, and its version. |
| `root` | The absolute dataset folder that every path is relative to, or `null` when it was not known: a `.spitout` resolved without `--root`. |
| `external_inputs` | Every artifact a job reads but no job writes, once each: the sources, and the outputs of stages left out. Ordered by path in natural order, the order `many` inputs take, so `wave2` comes before `wave10`. |
| `targets` | Every artifact a job writes but no job reads: what a full run leaves behind. Ordered by the job that writes it. |
| `executables` | The program each command and `verify` command starts with, once each, in text order. A command whose first word is a path names no program and is left out. A backend can check these are installed before running anything. |
| `removed` | What the input stage left out of the dataset, and why: see [Removal](#removal). `[]` when nothing was. |
| `left_out` | Outputs whose jobs could not be planned, each with its reasons: see [Left out](#left-out). `[]` for a complete plan. |
| `jobs` | Every job, each after the jobs it depends on. |

## Artifact

Every artifact is written the same way, wherever it appears:

```json
{"product": "cleaned", "entities": {"sub": "01", "ses": "1"}, "type": {"name": "Image", "args": []}, "path": "derivatives/cleaned/sub=01__ses=1.txt"}
```

| Field | Holds |
| --- | --- |
| `product` | The product's name. An imported product keeps its prefix, as in `text::shard`. |
| `entities` | Each dimension and its value, as strings, in the product's declared order. A product with no dimensions has `{}`. |
| `type` | `null` for an untyped product; `{"name": N, "args": [TYPE, ...]}` for a named type, with its arguments; `{"variable": V}` for a type variable left unbound. |
| `path` | The artifact's file, relative to `root`. |

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
  "verify": [[ARGUMENT, ...], ...]
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

### Commands

A command is a list of arguments, and each argument is a list of parts, joined with nothing between them. A part is either literal text, a JSON string, or an artifact's path, `{"path": P}` with `P` relative to `root`. The template `tool --in={raw} -o {output}` becomes:

```json
[["tool"], ["--in=", {"path": "in/1.txt"}], ["-o"], [{"path": "out/1.txt"}]]
```

That is four arguments: `tool`, `--in=in/1.txt`, `-o` and `out/1.txt`. A `many` placeholder becomes one argument for each artifact in its collection.

Run each argument as one word, exactly as given: no shell is involved, so nothing in it is split, expanded or interpreted. Resolve each path against `root`, or run the command from `root`, where the relative paths name the right files.

### Running `verify`

A backend runs a job's `verify` commands, in order, before its `command`. If one fails, the job does not run, and neither does any job that depends on it, directly or through others.

## Fingerprint

A job's fingerprint is a 64-bit FNV-1a hash of the compact JSON of what it reads, writes and runs: its `operation`, `inputs`, `outputs`, `command` and `verify`, each artifact with its product, entities, type and path. Its `id`, `stage`, `depends_on` and `dependents` are left out, so the fingerprint follows the work, not where the job falls in the plan.

The fingerprint changes when the job's command, its files, or which artifacts it reads or writes change. It does not read the files themselves, so it does not change when an input file's contents do; a backend that must rerun a job after its inputs change combines the fingerprint with its own record of the files, such as their modification times or hashes.
