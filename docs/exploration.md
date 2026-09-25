# Pipeline authoring trials

Three agents authored logical pipelines of different structures and complexity. All MRI terms in the ACT trial are example data; the compiler core remains domain agnostic.

| Pipeline | What it exercises | Command | Result |
| --- | --- | --- | --- |
| [Branching](../examples/branching.spit) | Shared policy, two processing branches, separate aggregations, recombination | `cargo run -- check examples/branching.spit` | 21 jobs |
| [Observed groups](../examples/rich_shapes.spit) | Several subjects and sessions, reference reuse, two successive aggregations, separate inventory | `cargo run -- check examples/rich_shapes.spit --sources examples/rich_shapes.sources` | 17 jobs |
| [Nested aggregation](../examples/complex.spit) | Partial types, irregular groups, reused inputs, three successive aggregations | `cargo run -- check examples/complex.spit` | 25 jobs |
| [MRtrix3 ACT](../examples/mrtrix3_act.spit) | Raw DWI runs, preprocessing, T1 registration, parcellation alignment, ACT and connectome | `cargo run -- check examples/mrtrix3_act.spit --sources examples/mrtrix3_act.sources` | 68 jobs |

The expanded ACT example starts with raw DWI runs, native T1w, reverse phase-encoded b=0 images, and two global lookup tables. It declares [denoising](https://userdocs.mrtrix.org/en/latest/reference/commands/dwidenoise.html), [Gibbs removal](https://userdocs.mrtrix.org/en/latest/reference/commands/mrdegibbs.html), [run concatenation](https://userdocs.mrtrix.org/en/latest/reference/commands/dwicat.html), [motion and distortion correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwifslpreproc.html), and [bias correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwibiascorrect.html). It then makes a session b=0 reference, declares rigid [T1 registration](https://userdocs.mrtrix.org/en/latest/reference/commands/mrregister.html) and [spatial transformation](https://userdocs.mrtrix.org/en/latest/reference/commands/mrtransform.html), and aligns an anatomically generated parcellation after [label conversion](https://userdocs.mrtrix.org/en/latest/reference/commands/labelconvert.html). The remaining stages suggest [5ttgen](https://userdocs.mrtrix.org/en/latest/reference/commands/5ttgen.html), [5tt2gmwmi](https://userdocs.mrtrix.org/en/latest/reference/commands/5tt2gmwmi.html), [dwi2response](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2response.html), [dwi2fod](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2fod.html), [ACT tractography](https://userdocs.mrtrix.org/en/latest/reference/commands/tckgen.html), [SIFT2](https://userdocs.mrtrix.org/en/latest/reference/commands/tcksift2.html), and [connectome construction](https://userdocs.mrtrix.org/en/latest/reference/commands/tck2connectome.html). These are logical dependencies, not executable command bindings. SPIT cannot inspect acquisition metadata, verify transforms, or assess image quality.

## Confirmed bug: partial type information depends on input order

This pipeline currently passes, though `Foo` and `Bar` are known to conflict:

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

The DAG reports `merged : Frame<Unknown>` and accepts `sink`. Reversing the `merge` arguments correctly rejects `Foo` versus `Bar`. An even stronger variant declares `merged : Frame<Bar>` and still produces `Frame<Unknown>`. The unifier accepts a later, more specific type but does not refine its earlier partial binding. This is a correctness bug in optional typing, not a request for stricter typing.

## Other limits exposed by authoring

1. **Mixed cardinality:** `combine(many Result, Policy)` is rejected because `many` must be an operation's only input. This blocks grouped results with a reusable configuration or reference.
2. **Selectors:** A family with an extra `revision` or `acq` dimension cannot be matched to a less specific driver, even when the author knows which revision to use. The parser accepts `@ vary(...)` only; `@ where(...)` and `@ same(...)` are not implemented.
3. **One output per operation:** The ACT example uses single-output response and FOD stages. Multi-output MRtrix3 commands such as multi-tissue response estimation cannot be represented as one invocation with several named products. This is a priority for the logical model: one job should own all named outputs, and the future Bash backend should bind a path for each.
4. **Collection contracts:** `many` accepts a one-item group. There is no operation-level minimum collection size or declared ordering. For values `1`, `2`, and `10`, the rendered input order is lexicographic: `1`, `10`, `2`.
5. **Validation scope:** With an empty inventory, `check` can print `Pipeline valid. 0 jobs resolved.` A declared branch with no source artifacts produces no jobs and no error unless a coverage rule makes that family required. Count rules can require two artifacts per observed group but cannot specify which entity values must be present. Fully unobserved groups remain unknowable by design.
6. **Input order and diagnostics:** The first `one` input determines a preserve operation's output dimensions; swapping otherwise equivalent ports can make a pipeline invalid. Errors identify positional ports as `input2`, which is hard to interpret in larger operations. Job ordering follows alphabetical dimension keys rather than product declaration order.
7. **Inventory override:** `--sources` selects the external inventory for resolution, but a malformed embedded inventory is parsed first and still fails. The CLI should either skip validation of overridden inline inventory or state that both must be well formed.
8. **Physical and execution layers:** SPIT cannot bind paths, check file existence or spatial compatibility, represent command options, or execute MRtrix3. The ACT example must remain a logical DAG until these layers exist. Bash generation belongs after DAG resolution and path binding; explicit command templates must not change which jobs exist.
