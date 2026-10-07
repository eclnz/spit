# Checks

Checks describe commands that a runner executes on individual artifacts. They do not inspect contents during compilation or change graph dependencies. Job-level input verification is defined under [operations](operations.md).

## Checks

A `check` tests one artifact once its file exists, with the tools that understand it. Declare it once, then attach it with `@ check(...)` where it applies:

```text
check ndim(n): check_ndim {@path} {n}
check nonempty: test -s {@path}

source t1w : Image .nii.gz [sub] @ check(ndim(3))

operation denoise(dwi: DWI @ check(ndim(4))) -> DWI .mif @ check(nonempty)
operation split(table: Table) -> (left: Table @ check(nonempty), right: Table)
```

`{@path}` is the artifact being checked, and the check must use it. Each `{param}` is the word given where the check is attached: `ndim(4)` runs `check_ndim` with the artifact's path and `4`. A check may use nothing else, so it reads one artifact and never adds a dependency, and it must use every parameter. A check with no parameters is written without parentheses, where it is declared and where it is attached. An argument is one word, with no spaces, quotes, braces, commas or parentheses. The command is split and quoted like any [command](operations.md#operations-and-commands).

`@ check(...)` follows an input port's type, an output's type and extension, or a source's dimensions, and may name several checks, as in `@ check(nonempty, ndim(3))`. The checks on an input port, and those of the source it reads, run on each artifact the port reads before the job's `verify` commands and command; a `many` port checks each artifact of its collection. The checks on an output run after the command, once the file exists, before the job counts as done. Several checks on one artifact all run; none replaces another. A step's product takes no checks: attach them to the operation's output.

SPIT does not run checks. It writes each job's checks into the `.spitdag`, bound to the artifacts they test (see [checks](dag.md#checks)), and a backend runs them. A failed check fails the job, even when its command succeeded, and the jobs that depend on it do not run. When the job that writes an artifact checks it after its command, a job in the same plan that reads it does not run the same check again. `spit dag --commands` shows each job's checks as `check:` lines, in the order they run.

Checks are global, as operations are. `use` brings in the checks of the operations and sources it imports, and `use ndim from checks.spit` imports a check by name. With `as`, an imported check takes the prefix too, as in `@ check(img::ndim(3))`; see [reuse](reuse.md#reuse-definitions).

### Default checks

When every output in a file or stage needs the same check, write one `check:` line instead of repeating `@ check(...)` on each output, as `path:` and `ext:` set a default for the products they cover:

```text
check nonempty: test -s {@path}
check ndim(n): check_ndim {@path} {n}

check: nonempty

stage preprocess:
    check: ndim(3)
    cleaned = denoise(raw)
```

A `check:` line outside every stage lists the checks run on every output of every step in it, stages included; one inside a stage lists them for the steps of that stage and the stages nested in it. A step takes the defaults of its own stage, not of the stage where its operation was declared, so a global operation called in two stages is checked by each stage's list. A default applies to every output of the step, each artifact of a named multi-output operation included. It never applies to an input port or a source: those keep the checks written at their `@ check(...)`.

Lists add up. A step's outputs run the file's checks, then those of each stage around the step from the outermost in, in the order written, and then the checks the operation's own output names. Where one check would run twice on an artifact, it runs once, at its first place. In the example above, `cleaned` runs `nonempty`, then `ndim(3)`.

To run less than a wider list sets, write `!` before the check. In a stage, `check: !nonempty` drops the file's `nonempty` for that stage's outputs and the stages in it, and a stage inside it may add it back. On an operation's output, `-> (empty_ok: Table @ check(!nonempty), rows: Table)` drops it for that output alone, wherever the operation is called. `!` names a check as written, `ndim(3)` included, and an output's own `@ check(nonempty)` is never dropped by it.

A file or a stage has one `check:` line. Its checks and the ones it drops must be declared checks, with the right number of arguments. A product may still be named `check`: `check = clean(raw)` and `check : Table [id] = clean(raw)` are steps, since a `check:` line has a list and no `=` outside parentheses. The `.spitdag` has no new fields: each default becomes a check on the artifact, after the job's command, beside the others, and `spit dag --commands` lists it as a `check:` line.
