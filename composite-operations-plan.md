# Composite operations plan

This plan settles the design of composite operations before any of it is built, as issue [#53](https://github.com/eclnz/spit/issues/53) asks. A composite is a reusable sequence of steps that a library declares once and a pipeline calls like an operation. Each call expands into ordinary jobs. The plan covers the syntax, how a call expands, how intermediate products are named, how a `.spitdag` records where each job came from, how diagnostics name both the call and the internal step, how checks combine, and how imported files are versioned. It ends with open questions that need a decision and the steps to build it.

Nothing here is built yet. Statements under [What SPIT does today](#what-spit-does-today) were checked against `spit` 0.2.2 built from `dev` at `d2d5b2f`. Everything after that section is proposed, and its sample output is what the design would print, not what SPIT prints now.

The plan lives at the repository root, as earlier plans did (`design-review-plan.md`, `folder-artifacts-plan.md`; `git log --diff-filter=D --name-only -- '*-plan.md'` lists them). `docs/` holds the user guide, which stays in the tree, so a plan does not belong there. Following `AGENTS.md`, the last commit before the feature merges deletes this file, after `docs/language-reference.md`, `docs/spitdag.md` and `docs/architecture.md` describe what was built.

## Decisions

These were settled after the plan was first written, and take precedence over anything below that reads otherwise.

- **No new keyword.** A composite is an operation whose header ends in `:` with an indented body of steps, in place of `command` lines. Both are operations: called the same way, imported the same way, in one namespace. An operation with a body and a `command` line is an error. Below, "composite" means such an operation.
- **No dimensions on ports.** A call whose argument lacks a dimension the body uses fails at the call anyway, since every expanded product maps to the call's step, so `[run]` on a port adds syntax for little.
- **Outputs are named.** An operation with a body writes its outputs `-> (name: Type, ...)`, since its steps assign them by name.
- **Calls show in the views that exist.** No `check --calls`: the commands view gains a `from:` line, `--counts` groups a call's steps, and hovers explain a call.
- **Step 1 is narrowed.** A body step records the file and line it is written at, which is all a composite needs. Placing every imported definition at its own line, for import errors in general, is separate work for its own issue.
- **Open questions 2 and 4 to 10** take the recommendations given under [Open questions](#open-questions); question 1 is answered by the first decision, and question 3 by the second.

## What SPIT does today

These were each run against the binary, on small files in a scratch folder.

**Imports.**

- `use lib/text.spit as text` brings in operations, sources, checks and `sidecars` groups, under `text::`. The path is relative to the importing file (`src/imports.rs`, `parse_document_at_inner`).
- A library's own steps are parsed and checked, but they are never imported. A library holding `sorted = sort_lines(shard)` adds no job to the pipeline that imports it.
- Imports nest. If `outer.spit` holds `use text.spit as T`, then importing `outer.spit as O` gives the names `O::T::shard` and `O::T::sort_lines`.
- An imported product keeps its qualified name in the `.spitdag` (`"product": "text::shard"`). `{@product}` writes it as `text.shard`, and `O::T::shard` as `O.T.shard`, for example in `d/T.shard/group=a__part=1.txt`.
- A path rule may name an imported product: `path text::shard: in/{group}/{part}.txt`.
- A job keeps the qualified operation name. `spit dag --commands` prints the header `Job 1  text::sort_lines  [pre]`, and the `.spitdag` has `"operation": "text::sort_lines"`.
- An imported check is qualified the same way: `"check": "text::nonempty"`.

**Product names.**

- An assignment cannot write a qualified product. `L::o = L::h(s)` fails with `expected type name`, and `a.b = h(s)` fails with ``invalid output product `a.b`; use letters, digits, and underscores``. A name with `::` can therefore never clash with a product the user writes.
- Two steps that assign one product fail with ``duplicate product name `o` ``.

**Where errors are reported.**

- An error inside an imported file is reported on the importing file's `use` line. The message holds the imported file's absolute path and its line, for example ``error: line 3, column 1: in `/…/lib3.spit` at line 3: operation `undeclared` must be declared before its first flow step``.
- An error in an imported operation's command, found once the operation has been merged, gives the `use` line and no file at all: ``error: line 3, column 1: command for `L::f` uses unknown placeholder `{y}` ``. Imported definitions are recorded at the `use` line's `Place` (`apply_import` in `src/imports.rs`). `Place` (`src/span.rs`) has a line and columns but no file.
- An error in a step that calls an imported operation is reported at that step, in the caller: ``error: line 4, column 10: type mismatch at `L::g.x`: product `s` is Lines, expected Table``.
- A resolve error is reported at the step too: ``error: line 9, column 12: no `t` artifact for input `b` of `two` at [id=1]``.

**Diagnostics in other files.**

- When a recipe's pipeline holds the error, `spit check r.spitin --json` gives the diagnostic a `"file": "main2.spit"` field, and the text form names the file: `error: main2.spit: line 4, column 10: …`. This is `Diagnostic::file` in `src/diagnostics/diagnostic.rs`.
- spit-vscode's `publishIssues` already puts a diagnostic with a `file` on that file, in its own collection. It reads only `severity`, `line`, `column`, `end_column`, `message` and `file`. Its hover renderer formats the details labelled `Used by`, `Command`, `Verify`, `Type bindings` and `Stage` as code.

**Checks add up.**

- A source's check and an identical check on the input port that reads it run once for each job.
- A check that the writing job runs on its output is not repeated by a job that reads that output.
- A different check on the reading port still runs.
- Two jobs that read the same source each run its "before" checks.

**The `.spitdag` and its runner.**

- The `.spitdag` is version 6.
- A job's `fingerprint` covers its `operation`, `inputs`, `outputs`, `command` and `verify` (`docs/spitdag.md`).
- spit-bash rejects unknown fields ("Invalid field types, unknown fields … are rejected", its README). So a new job field needs a new `version` and a change in spit-bash.

**Inspection commands.**

- `spit dag --counts` lists each step and its job count. On the ACT example (`examples/commands/mrtrix3_act/`) the preprocessing stage resolves to 48 jobs.
- `spit dag --jobs` lists each job's artifacts.
- `spit dag --commands` lists each job's commands and checks.

**Git can find a file's revision from its blob id.** `git hash-object examples/imports/text.spit` prints `2ccc264…`. Then `git log --all --find-object=2ccc264…` finds the commit `2df8205`, which wrote that exact content.

## Goals and limits

- **A call adds jobs; a declaration does not.** Importing or declaring a composite adds nothing to the DAG, just as importing a library's steps adds nothing today.
- **The author declares the interface, and SPIT infers the rest.** The library names the composite's input ports and public outputs. SPIT infers the dependencies between internal steps, as it does for any steps, from the products they read.
- **A call lowers to ordinary steps.** Following "Adding a feature" in `docs/architecture.md`, a composite call becomes plain `Invocation`s in lowering. Compile, resolve and bind treat them as any other steps, so selectors, types, `@ min`, `beside`, folders, `--partial` and checks work inside a composite without new code. The only new data is provenance: a table of calls, and a file and line for each step.
- **Calls never collide.** Each call's intermediate products carry the call's name, so their identities and paths differ from every other call's.
- **Nothing is hidden.** Every expanded job names its call, its composite, the file the composite comes from, and the content hash of that file. Every error inside a composite names the call site and the internal step.
- **There is no package registry.** Imports stay relative paths in the project, pinned by the project's own revision. See [Versions and imports](#versions-and-imports).
- **Out of scope at first:**
  - parameters that are not products, such as a number passed into a command;
  - stages, `path:` or `ext:` lines inside a composite's body;
  - recursion, which is an error;
  - merging identical work across calls.

## Syntax

### Declaring a composite

A composite is declared with a header like an operation's, followed by an indented body of steps, as a `stage` or `sidecars` block is written:

```text
operation name(port: Type @ check(...), ...) -> (out: Type @ check(...), ...):
    step
    step
```

**Ports.**

- Input ports are written as an operation's are: `name`, `name: Type`, `name: many Type`.
- A port may also list dimensions in brackets: `raw: MRI<DWI,Acquired>`. The list says which dimensions the body names. A product passed to the port must have at least those dimensions, and any others it has pass through, as with any step. The body names a dimension in `@ vary(run)`, `@ each(...)`, `@ where(...)` or `@ same(...)`, and the port must declare each one it names. This lets SPIT report a missing dimension at the call, rather than deep inside the body.
- A port may carry `@ check(...)`, as an operation's input port does.

**Outputs.**

- Outputs are always named, even when there is only one, because a step in the body assigns each one by name.
- An output may carry a type and `@ check(...)`.
- An output declares no extension and no `/`. The internal operation that writes it decides both.

**The body.** The body holds flow steps only: `product = operation(inputs)` and `a, b = operation(inputs)`, with selectors. A step may call an operation, or another composite that is declared earlier. The body reads its input ports and the products its own steps make. It must assign every public output exactly once, and it must not read a product from outside. The operations and checks it uses are declared at the top level of the library file, as now.

**Names.** Composites share the operation namespace. A call looks the same whichever one it names, so `foo` cannot be both an operation and a composite. A composite cannot be the target of a `command` or `verify` line, since it has no command of its own.

**Where to declare it.** A composite is declared at the top level, or inside a stage, where it stays global, as an operation does. It may be called in the file that declares it, or imported. `use` imports it by name, as it does an operation: `use clean_dwi_session from mrtrix_dwi.spit as mrx`.

**What an import brings.** Importing a composite also brings the operations and checks its body uses, transitively, under the same alias. This follows the rule that an operation brings its checks today (`select_import` in `src/imports.rs`). Collisions are reported as imports report them now.

### Calling a composite

A call is written exactly as an operation call, and it assigns one product to each public output, in order:

```text
corrected_dwi, session_b0 = mrx::clean_dwi_session(raw_dwi, dwi_bvec, dwi_bval, dwi_json, reverse_b0, reverse_b0_json)
```

The products on the left are the caller's own. They are public, and later steps read them by those names. A call may sit in a stage, and every job it expands into is then in that stage. A call takes the same selectors on its arguments as an operation call, and each selector applies where the body reads that port.

## Worked example 1: MRtrix diffusion preprocessing and registration

The library holds the reusable part of `examples/commands/mrtrix3_act/mrtrix3_act.spit`.

```text
# mrtrix_dwi.spit: a library of MRtrix3 diffusion steps
check ndim(n): mrinfo_ndim_is {@path} {n}

operation import_dwi(image: MRI<DWI,$Space>, bvec: GradientDirections, bval: GradientAmplitudes, metadata: AcquisitionMetadata) -> MRI<DWI,$Space>
command import_dwi: mrconvert {image} {@output} -fslgrad {bvec} {bval} -json_import {metadata}
operation import_reverse_b0(image: MRI<B0,Acquired>, metadata: AcquisitionMetadata) -> MRI<B0,Acquired>
command import_reverse_b0: mrconvert {image} {@output} -json_import {metadata}
operation denoise(mri: MRI<DWI,S>) -> MRI<DWI,S>
command denoise: dwidenoise {mri} {@output}
operation remove_gibbs(mri: MRI<DWI,S>) -> MRI<DWI,S>
command remove_gibbs: mrdegibbs {mri} {@output}
operation concatenate_runs(runs: many MRI<DWI,S>) -> MRI<DWI,S>
command concatenate_runs: dwicat {runs} {@output}
operation extract_b0(mri: MRI<DWI,S>) -> MRI<B0Series,S>
command extract_b0: dwiextract {mri} {@output} -bzero
operation mean_b0(mri: MRI<B0Series,S>) -> MRI<B0,S>
command mean_b0: mrmath {mri} mean {@output} -axis 3
operation combine_pe_pair(forward: MRI<B0,S>, reverse: MRI<B0,S>) -> MRI<B0Pair,S>
command combine_pe_pair: mrcat {forward} {reverse} {@output} -axis 3
operation preprocess_dwi(dwi: MRI<DWI,Acquired>, pe_pair: MRI<B0Pair,Acquired>) -> MRI<DWI,Diffusion>
command preprocess_dwi: dwifslpreproc {dwi} {@output} -rpe_header -se_epi {pe_pair} -align_seepi
operation bias_correct(mri: MRI<DWI,S>) -> MRI<DWI,S>
command bias_correct: dwibiascorrect fsl {mri} {@output}
operation export_nifti(mri: MRI<K,S>) -> MRI<K,S> .nii.gz
command export_nifti: mrconvert {mri} {@output}
operation flirt_register(moving: MRI<M,S>, reference: MRI<N,T>) -> FSLTransform<S,T> .mat
command flirt_register: flirt -in {moving} -ref {reference} -omat {@output} -dof 6 -cost corratio
operation convert_flirt(matrix: FSLTransform<S,T>, moving: MRI<M,S>, reference: MRI<N,T>) -> Transform<S,T> .txt
command convert_flirt: transformconvert {matrix} {moving} {reference} flirt_import {@output}
operation mrtransform(moving: MRI<K,S>, transform: Transform<S,T>, reference: MRI<N,T>) -> MRI<K,T>
command mrtransform: mrtransform {moving} {@output} -linear {transform} -template {reference} -interp linear

# One session's runs, cleaned, combined and corrected, with its mean b=0.
operation clean_dwi_session(raw: MRI<DWI,Acquired>, bvec: GradientDirections, bval: GradientAmplitudes, metadata: AcquisitionMetadata, reverse: MRI<B0,Acquired>, reverse_metadata: AcquisitionMetadata) -> (dwi: MRI<DWI,Diffusion> @ check(ndim(4)), b0: MRI<B0,Diffusion> @ check(ndim(3))):
    imported = import_dwi(raw, bvec, bval, metadata)
    reverse_mif = import_reverse_b0(reverse, reverse_metadata)
    denoised = denoise(imported)
    degibbsed = remove_gibbs(denoised)
    session = concatenate_runs(degibbsed @ vary(run))
    forward_series = extract_b0(session)
    forward = mean_b0(forward_series)
    pe_pair = combine_pe_pair(forward, reverse_mif)
    preprocessed = preprocess_dwi(session, pe_pair)
    dwi = bias_correct(preprocessed)
    b0_series = extract_b0(dwi)
    b0 = mean_b0(b0_series)

# Align an image with the diffusion data: FLIRT across contrasts, the matrix
# converted to MRtrix coordinates, then a linear resampling.
operation register_to_dwi(moving: MRI<K,S>, reference: MRI<N,T>, reference_nifti: MRI<N,T>) -> (aligned: MRI<K,T>, transform: Transform<S,T>):
    matrix = flirt_register(moving, reference_nifti)
    transform = convert_flirt(matrix, moving, reference_nifti)
    aligned = mrtransform(moving, transform, reference)
```

The pipeline then calls the session composite once and the registration composite twice. The second registration call takes a FLAIR image, a hypothetical source added to show two calls of one composite:

```text
# act.spit
use mrtrix_dwi.spit as mrx
path: derivatives/{@stage}/{@product}/{@entities}
ext: .mif

sidecars dwi [sub, ses, run]:
    source raw_dwi : MRI<DWI,Acquired> .nii.gz
    source dwi_bvec : GradientDirections .bvec
    source dwi_bval : GradientAmplitudes .bval
    source dwi_json : AcquisitionMetadata .json
sidecars reverse_b0_files [sub, ses]:
    source reverse_b0 : MRI<B0,Acquired> .nii.gz
    source reverse_b0_json : AcquisitionMetadata .json
source t1w : MRI<T1w,Anatomical> [sub, ses]
source flair : MRI<FLAIR,Anatomical> [sub, ses]

stage preprocess:
    corrected_dwi, session_b0 = mrx::clean_dwi_session(raw_dwi, dwi_bvec, dwi_bval, dwi_json, reverse_b0, reverse_b0_json)

stage anatomy:
    session_b0_nifti = mrx::export_nifti(session_b0)
    t1w_dwi, t1_to_dwi = mrx::register_to_dwi(t1w, session_b0, session_b0_nifti)
    flair_dwi, flair_to_dwi = mrx::register_to_dwi(flair, session_b0, session_b0_nifti)
```

**Jobs.** Over the ACT example's mock data (three sessions, with 2, 2 and 3 runs), the session call expands to the same 48 jobs as today's hand-written preprocessing stage. The registration calls add 3 jobs per step: 3 steps for each of the two calls, plus `export_nifti`. The reference is a port, so `session_b0_nifti` is made once and both calls share it. Had the composite exported the reference itself, each call would have repeated that work under its own name. That is correct but wasteful, and the library author avoids it by taking shared inputs as ports (see open question 5).

**Names.**

- The internal products of the first call are `corrected_dwi::imported`, `corrected_dwi::denoised`, …, `corrected_dwi::b0_series`.
- The internal products of the registration calls are `t1w_dwi::matrix` and `flair_dwi::matrix`. These two are distinct products, with distinct paths:

  ```text
  derivatives/anatomy/t1w_dwi.matrix/sub=01__ses=01.mat
  derivatives/anatomy/flair_dwi.matrix/sub=01__ses=01.mat
  ```

- The public outputs keep the caller's names and paths. For example, `corrected_dwi` is written to `derivatives/preprocess/corrected_dwi/sub=01__ses=01.mif`.

## Worked example 2: germline variant calling

The second domain is genomics. One sample's reads, split over sequencing lanes, are aligned, sorted, merged and duplicate-marked, then called to a GVCF. Several parts of SPIT that the MRtrix example does not use appear here: a `beside` output (the BAM index), an internal product that is not public (the metrics), and a check that adds up with an operation's own check.

```text
# germline.spit: per-sample alignment and GVCF calling with BWA, samtools and GATK
check nonempty: test -s {@path}
check bam_ok: samtools quickcheck {@path}

operation bwa_align(reference: Fasta, r1: Fastq, r2: Fastq) -> Sam .sam
command bwa_align: sh -c 'bwa mem "$1" "$2" "$3" > "$4"' sh {reference} {r1} {r2} {@output}
operation sort_reads(sam: Sam) -> Bam .bam @ check(bam_ok)
command sort_reads: samtools sort -o {@output} {sam}
operation merge_lanes(lanes: many Bam) -> Bam .bam
command merge_lanes: samtools merge -f {@output} {lanes}
operation mark_duplicates(bam: Bam) -> (marked: Bam .bam @ check(nonempty), index: BamIndex .bai beside marked, metrics: DupMetrics .txt)
command mark_duplicates: gatk MarkDuplicates -I {bam} -O {marked} -M {metrics} --CREATE_INDEX true
operation call_gvcf(reference: Fasta, bam: Bam, index: BamIndex) -> Gvcf .g.vcf.gz
command call_gvcf: gatk HaplotypeCaller -R {reference} -I {bam} -O {@output} -ERC GVCF

operation align_sample(reference: Fasta, r1: Fastq, r2: Fastq) -> (bam: Bam @ check(bam_ok), gvcf: Gvcf @ check(nonempty)):
    aligned = bwa_align(reference, r1, r2)
    sorted = sort_reads(aligned)
    merged = merge_lanes(sorted @ vary(lane))
    bam, index, metrics = mark_duplicates(merged)
    gvcf = call_gvcf(reference, bam, index)
```

```text
# somatic.spit
use align_sample from germline.spit as gl
path: results/{@product}/{@entities}

source reference : Fasta .fa
source normal_r1 : Fastq .fastq.gz [patient, lane]
source normal_r2 : Fastq .fastq.gz [patient, lane]
source tumour_r1 : Fastq .fastq.gz [patient, lane]
source tumour_r2 : Fastq .fastq.gz [patient, lane]

normal_bam, normal_gvcf = gl::align_sample(reference, normal_r1, normal_r2)
tumour_bam, tumour_gvcf = gl::align_sample(reference, tumour_r1, tumour_r2)
```

**Jobs.** For one patient `P01` with two lanes, each call expands to 7 jobs: 2 `bwa_align`, 2 `sort_reads`, and 1 each of `merge_lanes`, `mark_duplicates` and `call_gvcf`. That is 14 jobs in all.

**Names and paths.** The two calls have the same internal names, which do not collide:

```text
results/normal_bam.aligned/patient=P01__lane=L1.sam    results/tumour_bam.aligned/patient=P01__lane=L1.sam
results/normal_bam.merged/patient=P01.bam              results/tumour_bam.merged/patient=P01.bam
results/normal_bam/patient=P01.bam                     results/tumour_bam/patient=P01.bam
results/normal_bam/patient=P01.bai                     results/tumour_bam/patient=P01.bai
results/normal_bam.metrics/patient=P01.txt             results/tumour_bam.metrics/patient=P01.txt
```

**The index.** `index` is written `beside marked`, and `marked` is bound to the public `normal_bam`. So `normal_bam::index` takes its path from `normal_bam`'s path, as `beside` does today. The index is internal, yet it sits next to the BAM, where GATK looks for it.

**Checks.** On `normal_bam`, the operation's `nonempty` and the composite's `bam_ok` both run, as "after" checks of the `mark_duplicates` job. On `normal_bam::sorted`, the operation's `bam_ok` runs. The composite adds nothing there, because `sorted` is not a public output.

## Expansion

### Where it happens

Expansion happens in lowering (`src/lower.rs`), where a flow step becomes an `Invocation` today. When `add_flow_step` meets a call whose name is a composite, it expands the call in place of adding one invocation:

1. **Check the call against the signature.** This uses the existing arity and port checks. It also checks that each argument product has every dimension its port declares, so that a missing `run` is reported at the call's argument.
2. **Choose the instance name**, the call's first output product (see [Naming](#naming-each-calls-products)), and give the call a `CallId`.
3. **Expand each body step in order**, renaming its products:
   - an input port becomes the argument bound at the call, with the call's selectors kept;
   - a public output becomes the caller's product in that position;
   - any other product `p` in the body becomes `instance::p`;
   - a call to another composite expands in turn, with its own `CallId` whose parent is this call.
4. **Add each expanded step as an ordinary invocation**, through the existing `add_flow_step` path. That path infers the step's dimensions from its renamed inputs (`inferred_dimensions`) and declares its products. The call's stage becomes each step's `Invocation::stage`.
5. **Record provenance.** Each expanded step records where it is defined and which call made it (see [Data](#data)). `SourceMap::invocations` maps each expanded product to the call's `Step`, so every existing error that is located by product (`subject_place` in `src/diagnostics/places.rs`) points at the call with no further change.

Nested composites are expanded with an explicit worklist, not recursion, following "Loops and worklists, not recursion" in `AGENTS.md`. The nesting depth is capped, as `MAX_TYPE_DEPTH` caps types. A composite that calls itself, directly or through another composite, is an error that names the cycle. Expansion goes in its own module, `src/expand.rs`, so that `src/lower.rs` (504 lines) and `src/imports.rs` (538 lines) stay under the 800-line limit.

**Types.** The body's own steps unify their types per job, as now. On top of that, each call checks its boundary, in `src/compile/types.rs`, with fresh type variables per call:

- each argument's type unifies with its port's type;
- the type inferred for each public output unifies with the output's declared type.

A mismatch is reported at the call's argument, or at the output's name on the call's left side.

Everything after lowering sees ordinary invocations: compile (`src/compile`), resolve (`src/resolver`), bind (`src/paths/bind.rs`) and the writers. They need only to carry each step's provenance through to the `.spitdag` and the diagnostics.

### Naming each call's products

- **The instance name.** A call's instance name is its first output product, for example `corrected_dwi`, `t1w_dwi` or `normal_bam`. Product names are unique, so instance names are unique too. A call to a composite that is nested in another is named inside its parent: `outer_instance::inner_instance`.
- **Intermediate products.** An internal product is `instance::name`. A user cannot write such a name in an assignment today (`expected type name`), so it never clashes with a product the user writes. It could clash only with an imported source whose alias equals the instance name, so lowering rejects a call whose instance name is also an import alias.
- **Default paths.** `{@product}` already writes `::` as `.`, so the defaults keep calls apart: `results/{@product}/{@entities}` gives `results/normal_bam.aligned/…`. The path checks already reject a default rule that omits `{@product}`, so two calls never share a path.
- **Stages.** An internal product takes the call's stage, so `{@stage}` and `[{@stage}/]` work as for the call itself.
- **Path rules.** The caller may give any intermediate its own rule, `path normal_bam::aligned: …`, as `path text::shard:` works today. A library sets no paths. Where outputs go is the pipeline's choice, as for imported operations now.
- **Reading another call's intermediates.** Internal products are private. A step outside the call that reads `normal_bam::sorted` is an error that names the call and says to make it a public output. Internal products still appear in the DAG, in `--paths` and `--jobs`, and in spit-bash's `--product`.

### Data

Each change below names the existing type it extends.

| Change | Where | What |
| --- | --- | --- |
| `FileId(u32)` and `SourceFiles` | new, beside `src/span.rs` | A table of every file read while lowering: the pipeline, then each import. Each entry holds the file's path relative to the pipeline's folder, its text, and its blob id (see [Versions and imports](#versions-and-imports)). It is a newtype over `u32`, as `AGENTS.md` asks. |
| `FilePlace { file: FileId, place: Place }` | `src/span.rs` | A place in a given file. `Place` itself does not change. |
| `CompositeDef` | `src/model/definitions.rs` | Holds the name, the input ports (an `InputPort` plus its required dimensions), the output ports (name, type and checks), and the body. The body is a `Vec<Invocation>` over local names, with the annotated `ProductDef` of each step and each step's `FilePlace`. |
| `Pipeline::composites: Vec<CompositeDef>` | `src/model/pipeline.rs` | Lets `select_import` carry composites from one file to another, as it carries operations. |
| `CallId(u32)` and `Pipeline::calls: Vec<Call>` | `src/model/pipeline.rs` | A `Call` holds the composite's qualified name, the instance name, its `parent: Option<CallId>`, the call's `FilePlace`, and the composite's declaration `FilePlace`. |
| `Invocation::origin: Option<StepOrigin>` | `src/model/definitions.rs` | Set only on expanded steps. `StepOrigin` holds the `CallId` of the innermost call and the internal step's `FilePlace`. A step written in the pipeline has `None`, and its place is in `SourceMap`. |
| `DagStep::origin` and `BoundStep::origin` | `src/model/dag.rs`, `src/spitdag/mod.rs` | Copied once per step, never per job, following the performance rule "Find once, then look up". |
| `SourceMap::imports` | `src/parser/source_map.rs` | Gives each imported definition the `FilePlace` where it is written, not the `use` line, and keeps the `use` line beside it as the reason the definition is there. |

Imported operations, checks and sources gain the same `FilePlace`. This improves today's diagnostics on its own (build step 1), because an error in an imported command can then name its own file and line, rather than only the `use` line.

## Provenance in the `.spitdag`

The format becomes version 7, with two additions.

The document gains `pipeline_files`, the files the jobs came from:

```json
"pipeline_files": [
  {"path": "act.spit", "blob": "9f2c0e…"},
  {"path": "mrtrix_dwi.spit", "blob": "3b18e5…"}
]
```

**`pipeline_files`.**

- The first entry is the pipeline itself, named by its file name. The others are the files it imports, at any depth, each once, with paths relative to the pipeline's folder, in natural order.
- `blob` is the file's git blob id: the SHA-1 of `blob <length>\0<content>`, which is what `git hash-object <file>` prints. Anyone can check the id with `git hash-object`, and `git log --all --find-object=<blob>` finds the revisions that hold that exact text.
- Paths are relative, so the same pipeline gives the same bytes on every machine. This keeps the promise in `docs/architecture.md` that the output depends only on the pipeline, inventory, root and version.

Each job gains `origin`:

```json
"origin": {
  "step": {"file": 1, "line": 37},
  "operation": {"file": 1, "line": 8},
  "calls": [
    {"composite": "mrx::clean_dwi_session", "instance": "corrected_dwi", "at": {"file": 0, "line": 18}, "defined": {"file": 1, "line": 34}}
  ]
}
```

**`origin`.**

- `step` is the line that wrote the job's step.
- `operation` is the line of the operation's declaration.
- `calls` lists the calls that expanded into the step, outermost first, as `stage` does. Each call gives where it was made and where its composite is declared.
- `file` is an index into `pipeline_files`.
- For a step written in the pipeline, `calls` is `[]`.
- Every job has `origin`, including jobs with no composite. This means an imported operation's job also names the library file its command came from, which `operation` alone does not say today.

`origin` is left out of the fingerprint, as `stage` is: it records where a job came from, not the work it does. A library edit that changes a command already changes the fingerprints through `command`. A blob id that changes on its own, such as after an edit to a comment, reruns nothing.

The new version means a change in the runner. spit-bash validates every field and rejects unknown ones. Following "The local runner" in `AGENTS.md`, spit-bash gets a branch of the same name that:

- accepts version 7;
- prints the call beside a failed job, for example `[12] failed: … (in corrected_dwi = mrx::clean_dwi_session, act.spit line 18; mrtrix_dwi.spit line 37)`;
- may later choose jobs with `--call corrected_dwi`.

## Diagnostics

### Where an error is reported

The primary location of an error is always in the file being checked, so that an editor shows it there. A secondary location points into the library. There are three kinds of error.

1. **An error in the library's own text.** This is an error that does not depend on any call: an undeclared operation, a bad placeholder, a body that reads an outside product or leaves an output unassigned, or a cycle. `spit check mrtrix_dwi.spit` reports it at its own line. In a pipeline that imports the library, it is reported once, with `file` set to the library and its line and columns, and it is related to the `use` line. This replaces today's message, which gives the absolute path inside the text and its location on the `use` line.

2. **An error that depends on the call.** This is a type mismatch at a port, a missing dimension, an internal step whose join fails for these arguments, or a resolve gap such as a missing input. It is reported at the call. The message names the call's instance and composite, and the related location is the internal step in the library:

   ```text
   error: line 18, column 95: in `corrected_dwi = mrx::clean_dwi_session(…)`: no `reverse_b0` artifact for input `reverse` of `mrx::import_reverse_b0` at [sub=02,ses=01]
     --> mrtrix_dwi.spit: line 36, column 37: reverse_mif = import_reverse_b0(reverse, reverse_metadata)
   ```

   When the failing input of an internal step is one of the composite's ports, the primary columns are the argument at the call (`reverse_b0` above). That argument is what the caller can change. Otherwise the primary columns are the whole call. With nested calls, the primary location is the outermost call in the checked file, and there is one `-->` line for each call down to the internal step.

3. **An error in a public output.** A type that does not match the declared output, or a check on the output, is reported at that output's name on the call's left side. The related location is the output port in the composite's header.

`spit artifacts`, and the reasons in `dag --partial`'s `left_out`, use the same wording. A reason gains the call, for example ``in `corrected_dwi = mrx::clean_dwi_session(…)`: input `reverse` of `mrx::import_reverse_b0` needs …``.

### `check --json`

Each diagnostic may gain a `related` array. Each entry has the same fields as a diagnostic's location, plus a message:

```json
{"severity": "error", "source": "pipeline", "line": 18, "column": 95, "end_column": 105,
 "message": "in `corrected_dwi = mrx::clean_dwi_session(…)`: no `reverse_b0` artifact for …",
 "related": [{"file": "mrtrix_dwi.spit", "line": 36, "column": 37, "end_column": 44, "message": "internal step of `mrx::clean_dwi_session`"}]}
```

The field is additive. Today's extension ignores it and still shows the primary diagnostic, so the extension's change can follow the spit change rather than block it.

`--hovers` gains three things:

- On a call, the hover's details add `Composite: mrx::clean_dwi_session (mrtrix_dwi.spit line 34)` and `Expands to: …`. The second lists each internal step with its expanded product names, as `Used by` lists steps today.
- On an internal product named in a `path` rule, the hover gives its call and its internal step.
- On a `composite` header in a library, the hover gives its interface and the number of calls made to it in the file being checked.

`check --json` also lists each intermediate's resolved path in `paths`, at the call's line, so the path hints show where the call's internal files go.

### spit-vscode

The extension needs four changes, made on a branch of the same name, as "The VS Code extension" in `AGENTS.md` asks:

1. Map `related` to `vscode.DiagnosticRelatedInformation`, with each entry's file resolved as `publishIssues` resolves `file` now.
2. Add `Composite` and `Expands to` to the labels in `appendHoverDetail`, so they are shown as code.
3. Add the `composite` keyword to `syntaxes/spit.tmLanguage.json` and to the keyword list in its README.
4. Add tests in `extension.test.js` and `grammar.test.js`.

## Checks

Checks attach where they are declared, and every check that applies to an artifact runs. None replaces another. This is the rule that `docs/language-reference.md` states for checks today, carried into composites:

- **On an internal operation's port.** It runs as now, for every call.
- **On a composite's public output.** It attaches to the artifact that the body's step writes for that output. It runs as an "after" check of that job, after the operation's own output checks. In the `.spitdag`, its `check` is written as the pipeline attaches it, such as `gl::bam_ok`, so a failure names the check.
- **On a composite's input port.** It attaches as a "before" check to each internal job's port that reads the port. A source's checks attach the same way today. The existing rules then remove repeats: an identical check on the same artifact runs once per job, and a reader does not repeat a check that the writer in the same plan already ran.
- **Equality.** Two check uses are the same check when their qualified names and their arguments are equal, as `CheckUse` compares them now.

## Versions and imports

**Relative imports, pinned by the project's revision, are enough at first.** A library is a `.spit` file in the project, or vendored into it, for example as a copied folder or a git submodule, and `use` names it by a relative path, as now. The project's commit pins every library it holds. The `.spitdag` records each file's blob id, which is enough to:

- check that a plan came from the files in a given checkout: compare `git hash-object` on each file with `pipeline_files`;
- find which revision a plan's library came from: `git log --all --find-object=<blob>`;
- tell that a library changed between two plans, without diffing commands.

**What this plan does not include.**

- **No URL or registry imports.**
- **No version constraints in `use`.**
- **No reading of `.git` by SPIT.** SPIT records no revision itself: a working tree may hold uncommitted edits, and the blob id describes the text that was actually read.

A registry, or `use … @ <revision>`, can be added later without changing the `.spitdag`, because `pipeline_files` already identifies content.

**Messages give relative paths.** SPIT's messages show imported files by their path relative to the pipeline, not by the absolute canonical path that today's import errors print. The canonical path is still used to detect cycles.

## Acceptance test

The issue's test: starting from a short imported call, someone can inspect every job it expands into, and can follow an error back to the call and the exact internal definition without guessing. The test runs on the MRtrix example's mock data and on a germline fixture, each as an integration test in `tests/` with stored outputs under `tests/fixtures/outputs/`.

**1. The calls, before any data is read.** A new `spit check --calls` lists each call, its composite and file, and the steps it expands to:

```text
$ spit check act.spit --calls
corrected_dwi, session_b0 = mrx::clean_dwi_session(…)  line 18  [preprocess]
  composite mrx::clean_dwi_session  mrtrix_dwi.spit line 34  blob 3b18e5…
  line 35  corrected_dwi::imported = mrx::import_dwi(raw_dwi, dwi_bvec, dwi_bval, dwi_json)
  line 36  corrected_dwi::reverse_mif = mrx::import_reverse_b0(reverse_b0, reverse_b0_json)
  …
  line 46  session_b0 = mrx::mean_b0(corrected_dwi::b0_series)
t1w_dwi, t1_to_dwi = mrx::register_to_dwi(…)  line 22  [anatomy]
  …
Pipeline valid.
```

**2. Every job of one call.** `dag --counts` groups the steps under the call that made them:

```text
$ spit dag act.spitin --counts
jobs  step                                                 stage
      corrected_dwi, session_b0 = mrx::clean_dwi_session   preprocess
   7    corrected_dwi::imported = mrx::import_dwi           preprocess
   …
  48    in this call
   3  session_b0_nifti = mrx::export_nifti                 anatomy
   …
```

`dag --commands` adds a `from:` line to each job of a call:

```text
Job 12  mrx::denoise  [preprocess]
  from:   corrected_dwi = mrx::clean_dwi_session (act.spit line 18), mrtrix_dwi.spit line 37
  run:    dwidenoise derivatives/preprocess/corrected_dwi.imported/sub=01__ses=01__run=01.mif …
```

The same information is in the `.spitdag`, under each job's `origin`.

**3. Follow an error.** Removing `reverse_b0` for `sub=02,ses=01` from the mock data makes `spit dag act.spitin` fail with the message shown under [Where an error is reported](#where-an-error-is-reported). The message names the call's line and the argument's column in `act.spit`, and the internal step's line in `mrtrix_dwi.spit`. `spit check act.spit --json` on a type error, such as passing `t1w` as `raw`, gives the same two places through `related`.

**4. Calls do not collide.** In the germline fixture, `spit dag somatic.spitin --paths` lists `normal_bam.aligned` and `tumour_bam.aligned` paths for each lane. No path is shared, and the path checks pass.

**5. An import adds no jobs.** A pipeline that only imports `germline.spit` resolves no jobs. This already holds for steps, and a test records it for composites.

**6. Checks add up.** `spit dag somatic.spitin --commands` shows both `test -s` and `samtools quickcheck` as `check:` lines on the `mark_duplicates` job.

## Open questions

Each question needs a human decision. Each gives a recommendation.

1. **The keyword: `composite` or something else?** Alternatives are `workflow`, `subpipeline` or `procedure`. *Recommendation: `composite`.* It is the issue's word, it does not suggest a separate runtime the way `workflow` does, and it reads well beside `operation`.

2. **The instance name: the first output, or named explicitly?** *Recommendation: the first output by default, with an optional explicit name later if users ask for one.* The default needs no new syntax and is unique by construction. Its cost is that the second output's intermediates are filed under the first output's name, as `session_b0`'s steps are under `corrected_dwi::…`. An explicit name could be written `corrected_dwi, session_b0 = mrx::clean_dwi_session(…) as dwi_clean`.

3. **Must a port declare the dimensions the body names?** *Recommendation: yes.* `raw: … [run]` makes the interface say which dimensions the body relies on, and lets the error land on the caller's argument. It also lets `spit check` check a library alone, against the ports' declarations, without a call.

4. **Can the caller read a call's intermediate products?** *Recommendation: no, at first.* Only public outputs are an interface. If an intermediate is worth reading, the library should make it an output. Path rules for intermediates are still allowed, since where files go is the pipeline's choice.

5. **Should identical work in two calls be merged?** *Recommendation: no.* Each call owns its jobs. Merging would make one job belong to two calls and blur provenance. A library author takes shared work as a port, as `register_to_dwi` takes `reference_nifti`.

6. **The `.spitdag` hash: git blob id (SHA-1) or SHA-256?** *Recommendation: the git blob id.* `git hash-object` and `git log --find-object` both understand it, and that directly answers "which revision", given a pinned project revision. Either hash is a small hand-written function, and needs no new crate, so it keeps to "Two crates" in `AGENTS.md`. FNV-1a, which the fingerprint uses, is too weak to identify content that someone else might match on purpose.

7. **Should `origin` line numbers be in the `.spitdag`?** They change when someone edits a comment, which changes the file's bytes but not any fingerprint. *Recommendation: keep them.* They are what lets a reader follow a job to its line, and a rerun depends only on the fingerprint.

8. **Should the body allow stages?** *Recommendation: not at first.* A call's stage covers its jobs. Nested stages could be added later as `call stage/inner stage`, but `--stage` filters and `{@stage}` paths would then depend on the library's internal layout.

9. **Should composites take value parameters, such as a number passed into a command?** *Recommendation: no.* Operations do not take them either, and a variant is another operation. This needs its own issue if users need it.

10. **Should `use` without `as` import a composite?** It would bring the body's operations unqualified, where they could collide with the caller's own. *Recommendation: allow it, with the existing collision errors.* This matches how imports behave today.

## Build steps

Each step merges on its own, keeps output byte-for-byte the same unless it says otherwise, and updates the guide in the same commit, as `AGENTS.md` asks. A step that touches lowering, the model or the writers is benchmarked with `profiling/bench.py`. As each step lands, mark it done with its commit.

| # | Step | Repositories | Done |
| --- | --- | --- | --- |
| 1 | Parse an operation's body into `OperationDef::steps`, each step with its place, and check it where it is declared: its steps call operations declared before it, read only its ports and its own products, and assign every output once. Calling such an operation is an error until step 2. | spit | e11653b |
| 2 | Expand calls in lowering: instance names, renaming, selectors merged at ports, nested bodies by worklist, `Pipeline::calls` and `Invocation::origin`, call-site errors through `SourceMap::invocations`, private intermediates, output types from the declaration. | spit | e11653b |
| 3 | Import operations with bodies through `select_import`, with the operations and checks their steps use, qualified. | spit | e08c650 |
| 4 | Checks on a composite's ports and outputs, carried by the expanded steps and run by `step_checks`. | spit | e08c650 |
| 5 | Write `.spitdag` version 7, with `pipeline_files` and each job's `origin`, and the hand-written blob hash. | spit, spit-bash | 7b86d76; eclnz/spit-bash#3 |
| 6 | Show calls: `from:` in the commands view, grouping in `--counts`, call-aware reasons in `artifacts` and `left_out`, hovers, and `related` locations in diagnostics. | spit, spit-vscode | |
| 7 | Examples (MRtrix and germline), the guide, and the acceptance tests as stored outputs. | spit | |
| 8 | Move how composites work into `docs/architecture.md`, then delete this plan in a commit of its own. | spit | |

Step 5 is the only step that changes the `.spitdag` format.
