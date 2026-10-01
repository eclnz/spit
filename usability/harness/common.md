
## Your setup

You are a new user trying SPIT, a pipeline planner, on a real task. Everything you have is in your working folder, `__SANDBOX__`:

- `bin/spit`: the SPIT command-line tool. Call it by that full path, for example `__SANDBOX__/bin/spit help`.
- `GUIDE.md`: SPIT's user guide and language reference. Its linked documentation and example files are available under `docs/` and `examples/`. The complete worked examples are in `docs/examples.md`.
- `data/`: the dataset. All output paths above are relative to this folder.
- `REPORT.md`: a questionnaire to fill in at the end.

Rules:

- Stay inside your working folder. Do not read SPIT's source code, examples, or tests anywhere else on this machine, and do not search the web. Work from the guide, the tool's own help, and its messages, as a new user would.
- Do not change, rename, or delete the existing files under `data/`. You may add your own files anywhere in the working folder, including inside `data/`.
- The programs named in the commands do not exist on this machine. You are planning the jobs, not running them.
- Work as efficiently as a real user would; there is no time limit.

## Deliverables

1. `plan.spitdag` in the working folder, written by `spit dag ... -o`. It must hold exactly the jobs described above, with exactly those command lines and output paths.
2. Your pipeline file(s), left in the working folder.
3. `REPORT.md`, filled in candidly. Your report is how this study learns what to fix in SPIT, so problems and confusion are the most useful things you can record. Don't smooth them over.

Finish with a short reply giving your confidence (1–5) that `plan.spitdag` is exactly right, and your single biggest difficulty.
