# Pipeline authoring trials

The example pipelines exercise different structures and levels of complexity. MRI terms in the ACT example are domain-specific sample data; the compiler core remains domain agnostic.

| Pipeline | What it exercises | Command | Result |
| --- | --- | --- | --- |
| [Branching](../examples/pipelines/branching.spit) | Shared policy, two processing branches, separate aggregations, recombination | `cargo run -- check examples/pipelines/branching.spit` | 21 jobs |
| [Observed groups](../examples/pipelines/rich_shapes.spit) | Several subjects and sessions, reference reuse, two successive aggregations, separate inventory | `cargo run -- check examples/pipelines/rich_shapes.spit --sources examples/pipelines/rich_shapes.sources` | 17 jobs |
| [Nested aggregation](../examples/pipelines/complex.spit) | Partial types, irregular groups, reused inputs, three successive aggregations | `cargo run -- check examples/pipelines/complex.spit` | 25 jobs |
| [Selectors](../examples/pipelines/selectors.spit) | Pinned and partial matches, a verification, a two-output step, and a many input beside a single input | `cargo run -- check examples/pipelines/selectors.spit --sources examples/pipelines/selectors.sources` | 17 jobs |
| [Analytics joins](../examples/analytics/analytics.spit) | Five keyed joins, then day, customer, and tenant rollups | `cargo run -- check examples/analytics/analytics.spit` | 34 jobs |
| [MRtrix3 ACT](../examples/commands/mrtrix3_act.spit) | BIDS DWI import, preprocessing, T1 registration, parcellation alignment, ACT and connectome | `cargo run -- check examples/commands/mrtrix3_act.spit --sources examples/commands/mrtrix3_act.sources` | 93 jobs |

The expanded ACT example starts with BIDS NIfTI DWI runs, gradient and JSON sidecars, native T1w, reverse phase-encoded b=0 images and JSON sidecars, and two global lookup tables. It imports these images to `.mif`, then binds [denoising](https://userdocs.mrtrix.org/en/latest/reference/commands/dwidenoise.html), [Gibbs removal](https://userdocs.mrtrix.org/en/latest/reference/commands/mrdegibbs.html), [run concatenation](https://userdocs.mrtrix.org/en/latest/reference/commands/dwicat.html), [motion and distortion correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwifslpreproc.html), and [bias correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwibiascorrect.html). It generates a [SynthSeg parcellation](https://surfer.nmr.mgh.harvard.edu/fswiki/SynthSeg) and remaps its labels. For cross-contrast T1/b=0 alignment, it uses [FLIRT](https://fsl.fmrib.ox.ac.uk/fsl/docs/registration/flirt/user_guide.html), [transformconvert](https://userdocs.mrtrix.org/en/latest/reference/commands/transformconvert.html), and [mrtransform](https://userdocs.mrtrix.org/en/latest/reference/commands/mrtransform.html), including nearest-neighbor label resampling. The remaining stages bind [5ttgen](https://userdocs.mrtrix.org/en/latest/reference/commands/5ttgen.html), [5tt2gmwmi](https://userdocs.mrtrix.org/en/latest/reference/commands/5tt2gmwmi.html), [dwi2response](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2response.html), [dwi2fod](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2fod.html), [ACT tractography](https://userdocs.mrtrix.org/en/latest/reference/commands/tckgen.html), [SIFT2](https://userdocs.mrtrix.org/en/latest/reference/commands/tcksift2.html), and [connectome construction](https://userdocs.mrtrix.org/en/latest/reference/commands/tck2connectome.html). SPIT emits these command lines but cannot inspect acquisition metadata, verify transforms, or assess image quality.

The first ACT draft gave almost every processing stage a new type. The revised example shares `MRI<Kind,Space>` across image products and leaves state in product names. `extract_b0` and `mean_b0` are reused on acquired and corrected DWI, and `mrtransform` is reused on T1w and tissue images. A separate label resampling operation uses nearest-neighbor interpolation. SPIT supports the reusable signatures through type variables; nominal subtyping and independently checked state properties are still absent.

The analytics example exercises the same constraint mechanism outside imaging. Its relations use `Relation<...,CustomerKey>`, and each join operation repeats a key variable across both inputs. A relation declared with a different key type is rejected before job expansion. Entity dimensions then select the matching tenant and customer records, while explicit `vary(event)`, `vary(day)`, and `vary(customer)` calls change the result grain through successive rollups.

The ACT example uses the flow-first authoring form. Each source path sits beside its declaration, and a stage whose outputs share a format sets it once; the few outputs that differ within a stage have their own rule beside the assignment that produces them. Operation contracts and commands sit beside their first use; intermediate product declarations are inferred from the operation and its input shape. Explicit BIDS import, b=0 extraction, and FLIRT matrix conversion appear as separate DAG jobs. Its steps sit in nested stages: `preprocess` (import, denoise, combine, correct), `anatomy` (parcellation, registration, tissue), and `tractography` (fods, tracking, connectome), so `spit bash --stage tractography` runs the tractography once preprocessing and anatomy have run. Outputs land in a folder per stage through one `{stage}` default; the parcellation, response, tracking, and connectome stages set their own file format, and only the FLIRT inputs and outputs and the SIFT2 weights need a path rule of their own.

## Resolved bug: partial type information depended on input order

This pipeline previously passed, though `Foo` and `Bar` are known to conflict:

```text
products:
    partly : Frame<Unknown> [id]
    known : Frame<Foo> [id]
    merged [id]
    final : Frame<Bar> [id]
operations:
    merge(A, A) -> A
    sink(Frame<Bar>) -> Frame<Bar>
pipeline:
    merged = merge(partly, known)
    final = sink(merged)
sources:
    partly[id=x]
    known[id=x]
```

The unifier used to keep `merged : Frame<Unknown>` after seeing `Frame<Foo>`, so it accepted `sink`. Reversing the `merge` inputs changed the result. The unifier now refines an earlier partial binding with later known information, and this pipeline fails even with an empty inventory. A regression test covers the behavior.

## Limits exposed by authoring, and how they were resolved

1. **Mixed cardinality.** An aggregate operation may now pair its one `many` input with any number of single inputs: `combine(results: many Result, policy: Policy)` resolves with `combine(result @ vary(run), policy)`. Each single input is matched once per group, on the group's dimensions, and must not use a dimension the groups lack. An operation still takes at most one `many` input, because a job groups one collection.
2. **Selectors.** `@ where(revision=2)` keeps only the artifacts with that value and removes the pinned dimension from matching, so a family with an extra `revision` or `acq` dimension can join a less specific driver. `@ same(station)` matches an input on the listed dimensions alone; any other dimension must leave exactly one artifact per job, or resolution fails with an ambiguity error. Selectors combine, as in `frame @ where(acq=fast) @ vary(run)`. `@ each(scenario)` is the reverse of `vary`: it broadcasts an input over a dimension the driver lacks, such as running one forecast step for every scenario, and the outputs gain that dimension.
3. **Multiple outputs.** An operation may declare `-> (wm: Response, gm: Response, csf: Response)` and a step may assign `wm_response, gm_response, csf_response = estimate_responses(dwi)`. One job owns every output; each binds to its own product and path, and each output port name is a command placeholder. Downstream consumers of any output depend on that one job. The ACT example now estimates multi-tissue responses and FODs this way.
4. **Collection contracts.** `@ min(2)` rejects a group with fewer artifacts than the operation accepts. A collection is ordered by its product's declared dimensions, and runs of digits compare as numbers, so values `1`, `2`, and `10` arrive as `1`, `2`, `10`.
5. **Validation scope.** When an inventory is supplied, a source with no artifacts is reported with the steps it leaves empty, and any other step that resolves no jobs is reported on its own line. These are warnings, so `check` still passes when a branch is legitimately absent. A coverage rule can require entity values, as in `require reading day=1,2 per [station]`. Groups that no record or context mentions still cannot be seen; list them under `contexts:` to check them.
6. **Input order and diagnostics.** A preserve step is driven by the input with the most dimensions, so swapping equivalent ports never changes which jobs exist. Errors about a port name the product bound to it, as in ``no `reference` artifact for input `right` of `join` ``, and jobs follow declared dimension order rather than alphabetical keys.
7. **Inventory override.** With `--sources`, an inline inventory is skipped instead of parsed, and a warning points at it.
8. **Physical and execution layers.** `spit discover` builds an inventory from the files under `--root` whose paths match a source's path rule, and `check`, `dag`, and `bash` discover sources the same way when given `--root` and no inventory. SPIT stays domain agnostic, so it does not read image headers or command metadata itself; instead a `verify operation:` command runs before each of that operation's jobs, with the job's input paths, and stops the script if it fails. Use it for checks such as spatial compatibility with the tools that understand the files.
