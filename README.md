# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

**Describe an analysis once. Run it on any dataset. See every job, and every gap, before anything runs.**

Research data is ragged. One subject is missing a scan, one sample has a corrupt run, one configuration has fewer seeds than the rest. A loop over files, or a workflow tool driven by filename patterns, finds that out hours into a run, or quietly skips the work and leaves you to notice.

SPIT is a compiler for batch pipelines over files. You name the kinds of files you have, the dimensions they vary along (subject, run, sample, seed), and the commands that turn one into another. SPIT matches that against the files on disk, checks it, and hands back the exact list of jobs. When something can't be made, it says which file is missing and which line needs it.

## See it work

A small study: three subjects, each with functional runs, one without an anatomical scan.

<!-- spit-doc: plain -->

```text
data/
├── sub-01/  anat.nii.gz  bold_run-1.nii.gz  bold_run-2.nii.gz
├── sub-02/               bold_run-1.nii.gz
└── sub-03/  anat.nii.gz  bold_run-1.nii.gz
```

The pipeline, `study.spit`, says what to do for any subject, and nothing about these three:

```spit
root data
default path: derivatives/sub-{sub}/{@product}[_run-{run}].nii.gz

source anat : T1 [sub]
path anat: sub-{sub}/anat.nii.gz
source bold : Bold [sub, run]
path bold: sub-{sub}/bold_run-{run}.nii.gz

operation skullstrip(image: T1) -> T1
command skullstrip: bet {image} {@output}
brain = skullstrip(anat)

operation register(bold: Bold, anat: T1) -> Bold
command register: flirt -in {bold} -ref {anat} -out {@output}
aligned = register(bold, brain)

operation average(runs: many Bold) -> Bold
command average: fslmerge -t {@output} {runs}
mean = average(aligned @vary(run))
```

`[sub, run]` names what each file varies by. Steps join their inputs on the dimensions they share, so each run is registered to its own subject's brain, and `@vary(run)` gathers a subject's runs into one average. Ask SPIT for the plan:

<!-- spit-doc: output
mkdir -p data/sub-01 data/sub-02 data/sub-03
touch data/sub-01/anat.nii.gz data/sub-01/bold_run-1.nii.gz data/sub-01/bold_run-2.nii.gz data/sub-02/bold_run-1.nii.gz data/sub-03/anat.nii.gz data/sub-03/bold_run-1.nii.gz
echo '$ spit dag study.spit'
spit dag study.spit 2>&1 || true
echo '$ spit dag study.spit --partial --commands'
spit dag study.spit --partial --commands 2>&1
--- study.spit = @previous
-->

```text
$ spit dag study.spit
error: line 15, column 26: no `brain` artifact for input `anat` of `register` at [run=1,sub=02]
  1 more artifacts cannot be produced; run `spit artifacts` to list them, or `spit dag --partial` to plan the rest
$ spit dag study.spit --partial --commands
note: planned 7 jobs; left out 2 artifacts that cannot be produced (see left_out)
Job 1  skullstrip
  run:    bet sub-01/anat.nii.gz derivatives/sub-01/brain.nii.gz
Job 3  register
  run:    flirt -in sub-01/bold_run-1.nii.gz -ref derivatives/sub-01/brain.nii.gz -out derivatives/sub-01/aligned_run-1.nii.gz
Job 6  average
  run:    fslmerge -t derivatives/sub-01/mean.nii.gz derivatives/sub-01/aligned_run-1.nii.gz derivatives/sub-01/aligned_run-2.nii.gz
```

Nothing has run yet, and none of `bet`, `flirt` or `fslmerge` needs to be installed to get this far. `spit dag study.spit -o study.spitdag` saves the plan, and a runner executes it.

## Why SPIT

**Mistakes are compile errors, not failed jobs.** Types, dimensions, command placeholders and output paths are checked before any data is read. Pass the arguments of `register` in the wrong order, or name a placeholder that a job has many values of, and `spit check` points at the line:

<!-- spit-doc: output
mkdir data
sed 's/register(bold, brain)/register(brain, bold)/' study.spit > swapped.spit
sed 's/{@output} {runs}/{@output} {run}/' study.spit > typo.spit
spit check swapped.spit || true
spit check typo.spit || true
--- study.spit
root data
default path: derivatives/sub-{sub}/{@product}[_run-{run}].nii.gz

source anat : T1 [sub]
path anat: sub-{sub}/anat.nii.gz
source bold : Bold [sub, run]
path bold: sub-{sub}/bold_run-{run}.nii.gz

operation skullstrip(image: T1) -> T1
command skullstrip: bet {image} {@output}
brain = skullstrip(anat)

operation register(bold: Bold, anat: T1) -> Bold
command register: flirt -in {bold} -ref {anat} -out {@output}
aligned = register(bold, brain)

operation average(runs: many Bold) -> Bold
command average: fslmerge -t {@output} {runs}
mean = average(aligned @vary(run))
-->

```text
error: line 15, column 20: type mismatch at `register.bold`: product `brain` is T1, expected Bold
error: line 18, column 40: command for `average` uses `{run}`, but `runs` collects `aligned` across `run` with `@vary(run)`, so each job has many values of it; `{runs.run}` gives each artifact's, and the jobs of `mean` have `sub`
```

**Gaps in the data are explained, not discovered.** `spit artifacts` lists everything that can't be made and why, tracing each gap back to the missing file. `--partial` plans the rest, and the saved plan records what it left out.

**The pipeline stays clean of the dataset's quirks.** Which files to read, which subjects to exclude, and how many runs each must have go in a recipe (`.spitin`) beside the data, not in the pipeline. The same pipeline runs on the next dataset unchanged.

**Iterate on the data and the pipeline without starting over.** [spit-bash](runner/README.md), the runner, records what each job read, ran and wrote, and on the next run does only the work that changed:

- Add a subject or a run, and only its jobs and the collections it joins run. A dataset still being collected runs with `--partial`, and the subjects not yet complete wait.
- Edit a command, and only that operation's jobs rerun, with the steps after them.
- Change an input in a way that leaves a step's output the same, and the run stops there: the jobs after it are kept.
- `touch` a file, check it out again, or move the dataset, and nothing reruns, since files are compared by content.
- Rename a product or a dimension, and `--move-files` moves the existing outputs to their new paths rather than remaking them.
- Every output a run replaces is kept, and `spit-bash restore` puts it back. `spit-bash plan` says what will run and why before anything does.

**The plan is a file you can read.** A `.spitdag` holds every job with its inputs, outputs and command line, in a [documented format](docs/spitdag.md). spit-bash runs it on one machine, in parallel. Another runner, for a cluster or a cloud, needs nothing but the format.

**It is fast.** Planning 72,000 jobs for 1,000 subjects takes about a quarter of a second, and checking the pipeline a few milliseconds, so the editor can check as you type.

## Is it for you?

SPIT fits work where the same command-line tools run over many subjects, samples, files or configurations, and where each step reads files and writes files: neuroimaging, genomics, audio, geospatial tiles, parameter sweeps. [spit-showcase](https://github.com/eclnz/spit-showcase) has six pipelines on public data, from BIDS quality control to variant calling to a scikit-learn sweep.

It is not a fit for:

- **Plans that change as they run.** SPIT decides every job up front. A job can read a number from a file when it starts, but its output can't decide which jobs exist.
- **In-process code.** Steps are commands. A Python function is a script that the command calls.
- **Clusters and clouds, today.** The one runner runs jobs on a single machine.

SPIT is young: it is at version 0.5, and syntax still changes between minor versions.

## Install

SPIT needs the [Rust toolchain](https://www.rust-lang.org/tools/install), and spit-bash Python 3.10 or newer:

<!-- spit-doc: fragment -->

```sh
cargo install --git https://github.com/eclnz/spit --branch dev spit
python3 -m pip install "git+https://github.com/eclnz/spit@dev#subdirectory=runner"
```

Then plan and run a pipeline:

<!-- spit-doc: fragment -->

```sh
spit check study.spit                  # check the pipeline; reads no data
spit dag study.spit --counts           # jobs per step
spit-bash run study.spit -j 4          # run them, four cores at a time
```

The [VS Code extension](https://github.com/eclnz/spit-vscode) shows the same errors as you type, and on hover the type, dimensions, command and output path of each product.

## Learn more

- [Getting started](docs/getting-started.md) walks through a pipeline in this repository.
- [The guide](docs/guide/concepts.md) covers pipelines, matching, paths, recipes and finding why a job is missing.
- [Examples](docs/examples.md) include a ragged parameter sweep and a cohort with missing inputs.
- The [language manual](docs/language-reference.md), [command line](docs/cli.md) and [DAG format](docs/spitdag.md) are the references.

To browse the documentation as a site, install [uv](https://docs.astral.sh/uv/) and run `uv run properdocs serve`.

## Contributing

Issues and pull requests are welcome. [The architecture map](docs/architecture.md) shows how the compiler is laid out, and [AGENTS.md](AGENTS.md) holds the rules for the code and the checks to run before a commit.

## Disclaimer

SPIT was developed with the help of generative AI tools. I am not a Rust developer: most of the code and documentation was generated by AI, and I have checked it by testing its behavior rather than by expert review of the Rust itself. The test suite covers the examples and language features, but the code may still contain errors or unidiomatic Rust. The commands in a `.spitdag` run on your system when a backend runs them: review them before running them, especially on data you cannot easily replace. The software is provided as is, without warranty of any kind; see the [license](LICENSE).

## License

SPIT is released under the [MIT License](LICENSE).
