# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

SPIT is a language and compiler for building data pipelines from command-line tools. It works out which files belong together and which jobs to run when you apply the same analysis across many samples, sites, or days.

In a pipeline, you describe what each step reads and produces, the command it runs, and how the files are organised. Dimensions such as `site` and `day` give SPIT the information it needs to match inputs, repeat steps, and gather results.

Point the pipeline at a dataset and SPIT produces a plan with the jobs, their dependencies, and the exact commands. Inspect the plan before handing it to `spit-bash` to run.

You work with four things. A **pipeline** (`.spit`) says what to make, for any dataset. A **recipe** (`.spitin`) says where one dataset is and what to leave out of it. The **plan** (`.spitdag`) lists every job and command that `spit` resolves from the two. SPIT only plans: the **runner**, `spit-bash`, carries the plan out on your machine and skips jobs that are already current.

## What it looks like

A weather network has several stations in each region. Each station writes one readings file per day and has its own calibration file. The goal is one temperature series per region. To get there, clean each station’s readings with its calibration, then combine the cleaned series from the stations in that region.

<!-- spit-doc: setup
default path: [{region}/][{site}/]{@product}[_{day}].csv
-->

```spit
source reading : Readings<Raw> [region, site, day]
source calibration : Calibration [region, site]

operation clean(days: many Readings<Raw>, calibration: Calibration) -> Readings<Clean>
command clean: python3 clean.py --calibration {calibration} -o {@output} {days}

operation average(series: many Readings<S>) -> Readings<S>
command average: python3 average.py -o {@output} {series}

cleaned = clean(reading @vary(day), calibration)
regional = average(cleaned @vary(site))
```

Each `command` is an ordinary command line, and `{days}` and `{@output}` become file paths. `@vary(day)` gathers each station’s daily files into one job, and SPIT matches its calibration on `region` and `site`. `@vary(site)` then gathers the cleaned station series within each region. The `average` operation computes an hourly median.

[Run the weather example](https://github.com/eclnz/spit-showcase/tree/main/weather)

## What you can do

- **Reuse the same pipeline across datasets.** A recipe supplies data paths, exclusions, and coverage requirements.
- **Inspect the plan before running it.** See the jobs, inputs, and commands, and trace missing outputs to their causes.
- **Catch wiring mistakes early.** Types are optional. Declare them at the boundaries you want SPIT to check, and a step wired to the wrong input is an error before anything runs.
- **Reuse current outputs.** `spit-bash` skips jobs whose work is still current and reruns jobs when their inputs or commands change.

**Is it for you?** SPIT fits work where command-line tools run over many files, each step reading files and writing files. It isn't a fit when:

- you need the cloud today: spit-bash runs on one machine or through Slurm on a cluster, and other runners don't exist yet;
- steps are Python functions passing data in memory, or read from databases or streams;
- what runs next depends on what a step finds: the plan is fixed before anything runs.

## Install

Install the compiler with Rust’s Cargo and the runner in a Python 3.10 or newer environment.

<!-- spit-doc: fragment -->

```sh
cargo install --git https://github.com/eclnz/spit --branch dev spit
python3 -m pip install "git+https://github.com/eclnz/spit@dev#subdirectory=runner"
```

Then plan and run a small pipeline from this repository, which needs only `sort`:

<!-- spit-doc: fragment -->

```sh
git clone https://github.com/eclnz/spit && cd spit
spit dag examples/runnable/lines/lines.spitin --counts
spit-bash run examples/runnable/lines/lines.spitin -j 3
```

The [VS Code extension](https://github.com/eclnz/spit-vscode) shows errors as you type, and hovers show a product’s type, dimensions and output path. See [the editor](docs/guide/editor.md).

## Documentation

- [First run](docs/guide/first-run.md) installs SPIT and runs a pipeline, and [a weather network](docs/guide/walkthrough.md) reads a real analysis from data to figure.
- The guide goes on with [artifacts](docs/guide/artifacts.md), [operations](docs/guide/operations.md), [pipelines](docs/guide/pipelines.md), [datasets](docs/guide/datasets.md), [plans](docs/guide/plans.md) and [runs](docs/guide/runs.md), and has a page of [troubleshooting](docs/guide/troubleshooting.md).
- [The weather family](docs/guide/weather.md) is four runnable versions of one analysis that together use every feature of the language.
- [The command line](docs/cli.md) and [the DAG format](docs/spitdag.md) give each command's flags and the plan's fields.
- [spit-showcase](https://github.com/eclnz/spit-showcase) has analyses on public data: spoken digits, MRI, SARS-CoV-2 variant calling, elevation tiles and scikit-learn models.

To browse the documentation as a site, install [uv](https://docs.astral.sh/uv/) and run `uv run properdocs serve`.

## Contributing

Issues and pull requests are welcome. Start with [the architecture map](docs/architecture.md) to see how the compiler fits together, and [AGENTS.md](AGENTS.md) for the code rules and the checks to run before you commit.

## Disclaimer

SPIT was built with the help of generative AI. I'm not a Rust developer: AI wrote most of the code and docs, and I've checked it by testing how it behaves, not by expert review of the Rust. The tests cover the examples and language features, but the code may still have bugs or unidiomatic Rust. The commands in a `.spitdag` run on your system, so review them before you run them, especially on data you can't easily replace. The software is provided as is, without warranty of any kind; see the [license](LICENSE).

## License

SPIT is released under the [MIT License](LICENSE).
