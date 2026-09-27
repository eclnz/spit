# SPIT pipeline style guide

This guide is for writing a `.spit` pipeline. It says how to compose a
pipeline and covers every piece of syntax you can use to do it. It does not
cover how SPIT resolves, binds, or compiles a pipeline internally — see
[architecture.md](architecture.md) if you need that. As a writer, you never
need to know it: describe the products, the operations, and the rules, and
SPIT works out the jobs.

## Mental model

A pipeline is a template, not a set of instructions to run in order. You
declare:

- **products** — families of artifacts, identified by name plus a set of
  entity dimensions (e.g. every `bold[sub,ses,run]`)
- **operations** — reusable contracts: input ports, output ports, and how
  cardinality/dimensions behave
- **calls** (assignments) — one operation applied to specific products,
  which also declares the derived product being produced
- **paths** — where each product's files live on disk
- **commands** — the literal executable and arguments for each operation
- **constraints** — coverage rules an inventory must satisfy

Given an inventory of concrete artifacts, SPIT resolves this template into
jobs. You never enumerate jobs yourself — add a shard to the inventory and
the corresponding job appears without touching the pipeline.

## File shape

Two equivalent forms exist. Prefer **flow-first**: write each declaration
next to where it's first used, in the order a reader would want to read it
(source, its path, the operation, its command, the call). Reserve the
**sectioned** form (`products:`, `operations:`, `pipeline:`,
`constraints:`) for small, self-contained examples where grouping by kind
aids a quick read.

Don't mix conventions within one file. A file with no `pipeline:`/assignment
steps is treated as a pure library of reusable definitions (see
[Imports](#imports-and-libraries)) — fine for shared operations and
sources, but keep such files free of one-off pipeline steps.

Comments start with `#` at the start of a word, exactly as in Bash:
`--color=#fff` and `'#run'` are ordinary arguments, not comments. A `#`
that ends a word (`{output}# note`) is kept as part of the word and SPIT
warns about it because it *looks* like a comment — put a space before it,
or quote it, if you mean to start one.

## Declaring products

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
```

- `source` declares a family of *input* artifacts — not a file, a family.
  One concrete artifact is `image[subject=A,visit=1,run=2]`.
- The type annotation (`: Image`) is optional. Names and entity bindings
  are what identify an artifact; types are an additive layer on top (see
  [Types](#types-optional)).
- Dimension list order matters: it's the order collections are sorted in
  and the order errors report bindings in. Pick an order and use it
  consistently for a given set of dimensions across the file.
- A product with no dimensions is written with an empty list, `[]` (a
  single global artifact, e.g. a lookup table).

Derived products don't need a separate declaration — an assignment
introduces one automatically:

```text
processed = process(image)
average = mean(processed @ vary(run))
```

Only write an explicit type/dimension header when it clarifies something
a reader can't otherwise infer:

```text
average : Image [subject, visit] = mean(processed @ vary(run))
```

**Naming convention:** name a derived product for what it *is* after the
step, not for the step that made it (`denoised`, not `denoise_output`).
Chain names so the pipeline reads as a sentence: `raw → denoised →
registered → mean`.

## Declaring operations and commands

```text
operation process(image: Image) -> Image
command process: process_tool --in {image} --out {output}
```

- Declare an operation before its first call.
- Name input ports (`image:`) whenever the command or an error message
  benefits from it. Unnamed ports still work: a single unnamed input is
  `{input}`; several are `{input1}`, `{input2}`, ... — but named ports are
  easier for the next writer and give clearer errors. Prefer naming ports
  once an operation has more than one input.
- `output` is reserved: it's always the placeholder for a single unnamed
  output, so you can't use it as an input port name.
- A command's first word must be an executable on `PATH` (or an
  executable path). SPIT emits the line as-is; it does not install or
  resolve that executable for you.
- Command templates are ordered words and arguments — not a shell
  pipeline. Words are split and quoted the way Bash would, every argument
  is passed literally, and `$`/backticks are **not** expanded. Write `{{`
  or `}}` for a literal brace.
- Every output placeholder must appear somewhere in the command. Every
  placeholder must name a real port. SPIT checks brace/quote balance and
  placeholder names at load time, before any inventory is read.

### Multiple outputs

Name each output; each name becomes its own placeholder and its own
product:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

One job produces all of them together; anything downstream that depends
on any one output depends on that whole job.

### Verification

Use `verify` for a domain check that must pass before a job's command
runs — e.g. confirming two images share a grid with the tool that
understands grids. SPIT itself never inspects file contents; `verify` is
where you delegate that to a real tool:

```text
verify register: check_same_grid {moving} {reference}
```

A `verify` command may reference inputs only (no `{output}` — there isn't
one yet). If it fails, the generated script stops before running the
job's real command. Use it for cheap, fail-fast sanity checks, not for
heavy processing.

## Calling operations

The call is the assignment:

```text
sorted = sort_lines(shard)
merged = merge(sorted @ vary(part))
```

Arguments follow the operation's declared port order.

### Preserve vs. aggregate steps

Two shapes of operation:

- **Preserve** (all `one` inputs): the input with the *most* dimensions
  drives the step — regardless of where it sits in the port list — and the
  output takes that input's dimensions. Every other input must use a
  subset of the driver's dimensions, or the pipeline is rejected before
  any inventory is read.
- **Aggregate** (one `many` input): call it with `@ vary(dimension)` to
  say which dimension is being collapsed across. The output drops that
  dimension. An operation can take at most one `many` input (a job groups
  one collection), but that `many` input can sit beside any number of
  `one` inputs, each matched once per group:

```text
operation summarise(days: many Series, policy: Policy) -> Summary @ drop(day) @ min(2)
summary = summarise(reading @ vary(day), policy)
```

The `@ drop(dimension)` on the operation and the `@ vary(dimension)` on
the call must name the same dimension — write them to visibly match.

`@ min(n)` on an aggregate operation rejects any group with fewer than
`n` artifacts. Add it whenever "at least N" is a real requirement of the
underlying command (e.g. don't average one file, don't diff a single
timepoint).

### Selectors

Selectors narrow what an input matches, and can be combined:

```text
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
frame @ where(acq=fast) @ vary(run)
```

- `where(dim=value)` — keep only artifacts with that value, and remove
  `dim` from matching entirely. Use this to pin one fixed choice (a
  calibration revision, a fixed acquisition) out of a family that has
  more dimensions than the rest of the step.
- `same(dim, ...)` — match on *only* the listed dimensions; every other
  dimension of that input must resolve to exactly one artifact per job
  (an ambiguity error if not). Use this when a shared reference has fewer
  meaningful dimensions than its filename suggests (e.g. one reference per
  station, filed under whatever day it happened to be captured).
- `vary(dim)` — only valid on the `many` input of an aggregate call;
  states which dimension the aggregation collapses.

Pick the selector that expresses the actual intent, not just whichever
makes resolution pass — `where` says "this exact value only," `same` says
"ignore these other dimensions for matching purposes."

## Paths

```text
path: results/{product}/{entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

- `path:` (no product name) sets the default template, used by any
  product without its own rule. Keep one default near the top of the
  file.
- `path <product>:` overrides the default for one product. Place it right
  beside that product's `source` line or its assignment — paths read best
  next to the thing they're for, not gathered in a separate block.
- Templates may use `{product}`, `{entities}` (all of that product's
  dimensions at once, in declared order), or any single declared
  dimension by name (`{subject}`, `{run}`, ...). A rule must account for
  every one of the product's dimensions — either via `{entities}` or by
  naming each one individually.
- A multi-output step's outputs are separate products; give each its own
  `path` line if the default doesn't fit all of them.
- For an imported product `alias::name`, `{product}` renders as
  `alias.name`.
- All paths are relative to `SPIT_ROOT` (an env var the generated script
  reads; unset defaults to the current directory).

Two different products resolving to the same path for the same entities
is an error, caught at load time — before any job exists. Run `spit paths`
to see which rule (explicit, default, or none) covers each product, and
add `--strict-paths` in CI if you want every product to require an
explicit rule.

## Constraints

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
require image run=1,2 per [subject, visit]
```

- `require <product> <cmp><n> per [dims]` checks a count within each
  *observed* group of those dimensions — it does not assert a total count
  across the whole dataset. A group that no source or context mentions
  simply isn't checked.
- `require <product> <dim>=<v1>,<v2> per [dims]` requires specific entity
  values to be present in each group (e.g. exactly the two required scan
  runs), optionally combined with a count.
- Use `contexts:` to force a group to be checked even when every one of
  its expected inputs happens to be missing (otherwise a fully-missing
  group is invisible to `require` and no error is raised for it):

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]
```

Write a `require` for every product where "processing runs even though a
required input is silently absent" would be a real bug you want caught,
not just for products that happen to have obvious counts.

## Types (optional)

Types are additive: leave them off entirely, add them to a few products
and operations, or type the whole pipeline. A **known** mismatch is
rejected; missing type information never blocks resolution on its own.

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```

- A bare single capital letter (`S`, `K`) is a local type variable, scoped
  to that operation's signature.
- Use a `$`-prefixed name (`$Kind`, `$SourceSpace`) when a single letter
  would be unclear — same meaning, just a longer name.
- An unprefixed multi-letter name (`World`, `Image`, `Frame<T1w,...>`) is
  a concrete type.
- Type variables are legal in operation signatures only — never in a
  `source`/product declaration.
- Prefer encoding *processing state* in the product name (`raw_dwi`,
  `denoised_dwi`) rather than inventing a new nominal type per stage, and
  reuse one parameterized type (`MRI<Kind,Space>`) across the stages that
  share shape. This lets one operation signature serve many call sites
  (e.g. `mrtransform(MRI<K,S>, Transform<S,T>, MRI<N,T>) -> MRI<K,T>`
  serving both anatomical and tissue images) instead of writing near-duplicate
  operations per product.

## Imports and libraries

```text
use text.spit as text
sorted = text::sort_lines(text::shard)

use shard, sort_lines from text.spit as text
```

- Path is relative to the file containing the `use` line.
- An imported operation brings its `command`; an imported source brings
  its `path` and coverage rules.
- Imports never bring pipeline steps or inventory records — only
  reusable definitions.
- `as text` prefixes every imported name (`text::shard`). Omit it to bring
  names into the current scope directly — do this only when there's no
  risk of collision with another import.
- An imported source keeps its prefixed name in `sources:`/the inventory
  too (`text::shard[...]`).
- Split out a library file once an operation or source is genuinely
  shared across two or more pipelines — don't pre-emptively factor a
  single-use pipeline into a library.

## Inventory

The pipeline is inventory-agnostic; supply concrete artifacts separately:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

- Inline (`sources:` in the same file) is fine for a small, self-contained
  example. For anything meant to be reused across datasets, keep the
  inventory in a separate `.sources` file and pass it with `--sources`.
- A separate `--sources` file always replaces an inline one (with a
  warning) — don't rely on both being merged.
- `spit discover pipeline.spit --root data` builds an inventory
  automatically from files under `data` that match your `path` rules —
  prefer this over hand-writing an inventory whenever your source layout
  is already path-rule-shaped.

## Checklist before calling a pipeline done

- Every `source` has a `path` rule (explicit or covered by a sensible
  default).
- Every product that could legitimately be silently missing has a
  `require`, and any group that could be *entirely* absent but still
  needs checking is listed under `contexts:`.
- Every aggregate call's `@ vary(...)` matches its operation's
  `@ drop(...)`.
- Selectors (`where`/`same`) say what you actually mean, not just
  whatever satisfies resolution.
- Run `spit check` (add `--sources`/`--root` once you have real data) —
  fix every error; read every warning and either resolve it or confirm
  it's expected (e.g. an intentionally unused source in a work-in-progress
  file).
- Run `spit paths` to confirm every product resolves to a path, with no
  collisions.
- Run `spit dag` / `spit bound-dag` to sanity-check the job count and
  shape against what you expected before generating a script.

## Reference: full syntax at a glance

| Syntax | Meaning |
| --- | --- |
| `source name : Type [dims]` | declare an input product family |
| `name = op(args)` | call an operation, declaring a derived product |
| `name : Type [dims] = op(args)` | call with an explicit output declaration |
| `operation op(port: Type, ...) -> Type` | single-output operation |
| `operation op(...) -> (a: Type, b: Type)` | multi-output operation |
| `operation op(...) -> Type @ drop(dim)` | aggregate operation, output drops `dim` |
| `operation op(...) -> Type @ drop(dim) @ min(n)` | aggregate with a minimum group size |
| `command op: exe {port} {output}` | executable + arguments for an operation |
| `verify op: exe {port}` | pre-job check using inputs only |
| `path: template` | default path rule |
| `path name: template` | path rule for one product |
| `{product}` `{entities}` `{dim}` | path template placeholders |
| `require name cmp per [dims]` | coverage rule, e.g. `count>=1` |
| `require name dim=v1,v2 per [dims]` | required entity values per group |
| `contexts:` | force-check groups with no matching source |
| `input @ vary(dim)` | mark the varying dimension on a `many` call arg |
| `input @ where(dim=value)` | pin a value, remove `dim` from matching |
| `input @ same(dim, ...)` | match on only the listed dimensions |
| `use file.spit as alias` | import all definitions under a prefix |
| `use a, b from file.spit as alias` | import selected definitions |
| `S`, `$Name` | type variables (single letter / `$`-prefixed) |
| `# comment` | line comment (word-initial `#` only) |

## Full example

```text
source shard : Lines [group, part]
require shard count>=1 per [group]

path: {product}/{entities}.txt
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines @ drop(part)
command merge: sort -m -u -o {output} {items}
merged = merge(sorted @ vary(part))
```

For more worked examples of specific mechanics (branching, nested
aggregation, joins/rollups, mixed cardinality, multi-output steps with
`verify`), see the table in the main [README](../README.md#more-examples).
