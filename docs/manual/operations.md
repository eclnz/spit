# Operations

An operation declares a reusable input/output contract. A step calls that contract with product families. Every operation name is user-defined; SPIT supplies no domain operations. The contract is valid without a command, although planning reports a warning because a runner cannot execute it.

## Operations and commands

```text
operation process(image: Image) -> Image
command process: process_tool --in {image} --out {@output}

operation mean(images: many Image) -> Image
command mean: mean_tool {images} --out {@output}
```

Declare an operation before its first use. Inputs in a call follow the port order in the declaration, and SPIT checks each product's type against that port. For example, with `operation compare(series: Series, policy: Policy)`, `compare(reading, policy)` uses `reading` as `series`; reversing the arguments is a type error when their types are known.

Matching, collections, and selectors are specified under [matching](matching.md).

An output's type may be followed by the extension the tool gives its file, as in `-> Transform .mat`; see [extensions](paths.md#extensions). An output the tool writes next to another without being told where is declared [`beside`](paths.md#files-a-tool-writes-beside-another) it.

An operation can write several outputs in one job. Name each output; its name is its placeholder, and the call assigns one product to each:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

A `verify` command checks a job's inputs before its command runs, using the tools that understand the files. SPIT does not run it; it writes each job's `verify` commands into the `.spitdag` beside its command, and a backend runs them first, in order. If one fails, the job does not run, and neither does any job that depends on it:

```text
verify register: check_same_grid {moving} {reference}
```

A `verify` command may use any input port, including a `many` one, which is filled in as in the command: `verify fit: validate_panel {waves}` checks every wave a fit job reads. `spit dag --commands` shows each job's `verify` lines above the command they guard, with their paths filled in.

A tool that takes a folder and a name instead of a path, and adds the extension itself, is given an output's folder with `{image.dir}`, and its file name without its extension with `{image.stem}`. Either counts as using the output. `.stem` needs the output to declare its [extension](paths.md#extensions), so that SPIT knows where the name ends, unless the output is a [folder](paths.md#folders), whose stem without one is its whole name:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -z y -b y -o {image.dir} -f {image.stem} {dicom}
```

For `derivatives/image/sub=01.nii.gz`, this passes `-o derivatives/image -f sub=01`. A file at the dataset root is in folder `.`. Only outputs have `.dir` and `.stem`, and `{@output.dir}` and `{@output.stem}` name the single unnamed output's. A placeholder with any other `.` part is an error.

Every input port has a name, and its placeholder is that name. A port is written `name`, `name: Type`, `name: many`, or `name: many Type`; a lowercase word alone, as in `operation copy(image)`, is an untyped port, and type names start with a capital letter. In `source reading : Series [station, day]`, `reading` is the product that identifies artifacts and `Series` is its type: write `operation compare(reading: Series)`, then call it with `compare(reading)`. `{@output}` is the path of a single unnamed output; its `@` marks a SPIT-supplied placeholder, while a named output such as `{wm}` uses the name in the operation declaration. `output` cannot name an input or explicit output port, and the old `{output}` spelling is an error that points to `{@output}`. A command must use every output placeholder, or its `.dir` or `.stem`, except an output written [`beside`](paths.md#files-a-tool-writes-beside-another) another; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection, since a backend runs a command without a shell. An unquoted `|`, `>`, `&&`, `;`, `2>&1` or the like is therefore an error; quote it (`'>'`) to pass it to the program as an argument. A command that needs a pipe or a redirection runs a shell itself and passes the paths to it as arguments, so that a path is never read as shell text: `command first: sh -c 'cut -f1 "$1" > "$2"' sh {table} {@output}`. The same holds for `verify` and `check` commands. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. As in Bash, text in single quotes is literal, so `awk '{print $1}'` needs no escaping; a placeholder is filled in unquoted text or double quotes. Write `{{` or `}}` for a literal brace elsewhere. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and each output must appear, as above.

Products, operations and dimensions have separate names, so a dimension may share a product's (`model @ each(model)`). A product may also share its operation's name, but `spit check` warns: name the result, as in `coregistered = coreg(mc, brain)`, so the step reads as what it makes.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {@output}
```

### Operations carried out by steps

An operation may be carried out by steps instead of a command: its header ends in `:`, and its steps are indented beneath it. A call to it looks like any other call, and becomes the body's steps over the caller's products, each with its own jobs:

```text
operation clean(x: Lines, t: Table) -> Lines
command clean: clean {x} {t} {@output}
operation merge(xs: many Lines) -> Lines
command merge: merge {xs} {@output}
operation count(x: Lines) -> Count
command count: wc {x} {@output}

operation summarise(reads: Lines, table: Table) -> (merged: Lines, total: Count):
    cleaned = clean(reads, table)
    merged = merge(cleaned @ vary(lane))
    total = count(merged)

first, first_total = summarise(raw, calibration @ where(revision=2))
```

The body reads the operation's inputs, by their port names, and the products its earlier steps make, and it makes each named output once. Its outputs are the products the caller names: here `first` and `first_total`. A product the body makes for itself is filed under the call's first output, so `cleaned` is `first::cleaned`, written `first.cleaned` in a path, and a second call of `summarise` files its own apart. So a call's first output cannot also be an import's alias: `use parts.spit as first` with the call above is an error at the call. Only the outputs are the caller's to read: a step that reads `first::cleaned` is an error that says to make it an output.

A selector the caller gives an input holds wherever the body reads it, beside the body's own: above, every `clean` job reads revision 2, and `merge` collects each group's lanes. A call in a stage puts every step it makes in that stage. A body may call another operation with a body, which is expanded in turn; every operation a body calls is declared before it. An output written with a type, as `total: Count`, gives the caller's product that type, and SPIT checks it against the step that makes it. An error in a step the call makes, such as a type the step does not accept or an input the data lacks, is reported at the call, named before the message. It points at the argument the caller gave when the failing input reads one of the operation's inputs, at the product the caller names when it is one of the outputs, and at the whole call otherwise. Below it, a `-->` line gives each call it is nested in and the body's step, with their file and line:

```text
error: line 4, column 21: in `m, t = L::summarise(...)`: type mismatch at `L::clean.x`: product `cal` is Table, expected Lines
  --> libs/lib.spit: line 13, column 15: the call of `L::tidy` in the body of `L::summarise`
  --> libs/lib.spit: line 10, column 9: the step in the body of `L::tidy`
```

`spit check --json` gives the same places as the diagnostic's `related` list. `spit artifacts` names the call beside each artifact a call's step cannot make, as ``(L::clean, in `m, t = L::summarise(...)` on line 8)``, and the reasons in a partial `.spitdag`'s `left_out` start with it.

A check on an input or output of such an operation, as `reads: Lines @ check(lines(2))`, runs on every step that reads that input or makes that output, beside the checks of the step's own operation; the same check on one artifact runs once. Imported, it brings the operations its steps call, and their commands and checks, under the same prefix, so `use summarise from lib.spit as L` brings `L::clean` too.

`spit check pipeline.spit --calls` lists each call's steps before any data is read. It shows the call, its line and stage, the file that declares the operation with its file's blob id, then each step with the line of the body that writes it; a call in a body is shown under its caller:

```text
m, t = L::summarise(…)  line 6  [report]
  L::summarise  libs/lib.spit  blob 3b18e5c
  line 13  m::cleaned = L::tidy(raw, cal)
    line 10  m::cleaned = L::clean(raw, cal)
  line 14  m = L::merge(m::cleaned)
  line 15  t = L::C::count(m)
```

With `--json` the same calls are the `calls` array of `{"diagnostics":[...],"calls":[...]}`, one object per call with its `steps`, a nested call having its caller's `id` as `parent`.

`spit dag --counts` lists a call's steps under it, with the call's jobs in all, so `summarise` over two groups of two lanes shows:

```text
jobs  step
      first, first_total = summarise
   4    first::cleaned = clean
   2    first = merge
   2    first_total = count
   8    in this call
   8  total
```

`spit dag --commands` starts each job a call made with a `from:` line: each call the job is nested in, outermost first, as `first = summarise (pipeline.spit line 19)`, then the file and line of the body's step that made it. The `.spitdag` holds the same, under each job's [`origin`](dag.md#where-jobs-come-from).

An operation with a body names its outputs, as `-> (result: Type)`, since its steps assign them by name. An output takes its extension, folder and place from the step that writes it, so the header gives only its name and type. Such an operation takes no `command` or `verify` line; its steps' operations have their own.
