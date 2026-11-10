# Plan: an output root apart from the dataset root (issue 185)

Issue: eclnz/spit#185. Branch `claude/dazzling-hawking-5ht5ns` in spit, spit-bash and spit-vscode.

## Goal

Derived files may go under a folder of their own, so a read-only dataset can be processed, two analyses of one dataset keep apart, and a later scan of the dataset does not see what a run wrote.

## Design

**Declaring it.** An `output <directory>` line mirrors `root`:

- In the pipeline or the recipe, not both. Relative to the file it is in.
- `--output-root <directory>` for a pipeline run alone, as `--root` is, relative to the working folder; an error beside a pipeline `output` line, a recipe or a `.spitout`. (`--output` already means `-o`.)
- A `.spitout` written by `spit inputs -o` records it as it records `root`, relative to the `.spitout`'s folder. A printed `.spitout` records neither. `dag` on a `.spitout` without one takes the pipeline's.
- Without one, outputs stay under the dataset root, as before.
- The folder need not exist: a runner makes it. `output` is a reserved word, so no product may be named `output`.
- An output root inside the dataset root is not scanned by `spit inputs`.

**Which root an artifact is under.**

- Sources, and outputs written `beside` an input that is under the dataset root, are under the dataset root.
- Every other job output is under the output root.
- A relative path stays unique across the two roots, and none is inside another's file or written folder. A runner may still key its records by path.

**The `.spitdag`, version 12.**

- Top-level `output_root`: the absolute output folder. It equals `root` when none is declared, and is `null` only when neither is known.
- Each artifact has `"under": "root"` or `"under": "output_root"`, naming the top-level field its `path` is relative to.
- Command parts, `stdin`, `stdout` and checks keep naming artifacts by path; since paths are unique, the artifact's `under` says which root. Commands run from `output_root`.
- `dag --commands` and `--paths` show a path under a separate dataset root reached from the output root, as `../data/in.txt`.

**The runner.** spit-bash resolves each path against its artifact's root, runs commands from `output_root`, makes it before the first job, and keeps `.spit-bash/` under `output_root`.

## Steps

1. [x] Language and command line: `output` in pipeline, recipe and `.spitout`; `--output-root`; the output root reaches `BoundDag`. Docs: language reference, CLI page, guide. (32862c3)
2. [x] Binding: each artifact's root (`Under`), computed beside its path in `bound_paths`; kept in `BoundDag`. (32862c3)
3. [x] `.spitdag` version 12: `output_root` and `under`; `dag` notes that commands run from the output root. Docs: `docs/spitdag.md`, `docs/architecture/binding.md`. (32862c3)
4. [x] Discovery: an output root inside the dataset root is not scanned. Docs: `docs/architecture/inputs.md`. (32862c3)
5. [x] Version 0.4.0, the first breaking change since 0.3.0. (b2eb430)
6. [x] spit-vscode: `output` keyword in the grammar and README; `--hovers` gains the `output` word; version 0.4.0. (eclnz/spit-vscode f58d4f1)
7. [x] spit-bash: read version 12, resolve paths by root, run from, make, and keep state under `output_root`; version 0.4.0. (eclnz/spit-bash e5cd623, d6c8abb, a731d75)
8. [ ] spit-showcase: use an output root where it shows the point.
9. [ ] Delete this plan before the merge.
