# Examples

Each pipeline below, under [`examples/`](../examples), checks cleanly and sits beside a `.spitin` recipe with its `require` rules and a `.spitout` of its inputs. Run the command from the repository root to see its jobs; add `--paths` to see each artifact's file or `-o plan.spitdag` to write them, or run `spit check` on the `.spit` or `.spitin` alone.

| Pipeline | Shows | Command | Jobs |
| --- | --- | --- | --- |
| [Branching](../examples/pipelines/branching.spit) | A shared policy, two branches with their own aggregations, and a recombination | `cargo run -- dag examples/pipelines/branching.spit examples/pipelines/branching.spitout` | 21 |
| [Observed groups](../examples/pipelines/rich_shapes.spit) | Several subjects and sessions, a reused reference, and two successive aggregations | `cargo run -- dag examples/pipelines/rich_shapes.spit examples/pipelines/rich_shapes.spitout` | 17 |
| [Nested aggregation](../examples/pipelines/complex.spit) | Partial types, irregular groups, and three successive aggregations | `cargo run -- dag examples/pipelines/complex.spit examples/pipelines/complex.spitout` | 25 |
| [Selectors](../examples/pipelines/selectors.spit) | `where`, `same`, a verification, a two-output step, and a many input beside a single input | `cargo run -- dag examples/pipelines/selectors.spit examples/pipelines/selectors.spitout` | 17 |
| [Analytics](../examples/analytics/analytics.spit) | Five keyed joins, then day, customer, and tenant rollups | `cargo run -- dag examples/analytics/analytics.spit examples/analytics/analytics.spitout` | 34 |
| [Stages](../examples/stages/stages.spit) | Preprocessing and analysis stages with `{stage}` paths | `cargo run -- dag examples/stages/stages.spit examples/stages/stages.spitout` | 7 |
| [Nested stages](../examples/stages/nested.spit) | Stages within a stage | `cargo run -- dag examples/stages/nested.spit examples/stages/nested.spitout` | 9 |
| [Field survey](../examples/commands/field_survey/field_survey.spit) | Sidecar files, calibration, alignment between spaces, and commands | `cargo run -- dag examples/commands/field_survey/field_survey.spit examples/commands/field_survey/field_survey.spitout` | 93 |
| [MRtrix3 ACT](../examples/commands/mrtrix3_act/mrtrix3_act.spit) | A diffusion MRI pipeline in nested stages, from BIDS import to connectome | `cargo run -- dag examples/commands/mrtrix3_act/mrtrix3_act.spit examples/commands/mrtrix3_act/mrtrix3_act.spitout` | 93 |

## Analytics

The analytics example applies the same matching outside imaging. Each relation is typed by its key, as in `Relation<...,CustomerKey>`, and each join repeats a key variable across both inputs, so joining relations with different keys fails before any job is created. Entity dimensions pick the matching tenant and customer records, and `vary(event)`, `vary(day)`, and `vary(customer)` change the grain through successive rollups. [`analytics_bad_join.spit`](../examples/analytics/analytics_bad_join.spit) shows the error for a mismatched key.

## MRtrix3 ACT

The ACT example starts from BIDS NIfTI DWI runs with their gradient and JSON sidecars, a native T1w image, reverse phase-encoded b=0 images, and two lookup tables. Its steps sit in three stages:

- `preprocess` imports each DWI to `.mif`, then runs [denoising](https://userdocs.mrtrix.org/en/latest/reference/commands/dwidenoise.html), [Gibbs removal](https://userdocs.mrtrix.org/en/latest/reference/commands/mrdegibbs.html), [run concatenation](https://userdocs.mrtrix.org/en/latest/reference/commands/dwicat.html), [motion and distortion correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwifslpreproc.html), and [bias correction](https://userdocs.mrtrix.org/en/latest/reference/commands/dwibiascorrect.html).
- `anatomy` makes a [SynthSeg parcellation](https://surfer.nmr.mgh.harvard.edu/fswiki/SynthSeg), aligns T1w to b=0 with [FLIRT](https://fsl.fmrib.ox.ac.uk/fsl/docs/registration/flirt/user_guide.html), [transformconvert](https://userdocs.mrtrix.org/en/latest/reference/commands/transformconvert.html), and [mrtransform](https://userdocs.mrtrix.org/en/latest/reference/commands/mrtransform.html), and builds the [tissue segmentation](https://userdocs.mrtrix.org/en/latest/reference/commands/5ttgen.html) and [seed interface](https://userdocs.mrtrix.org/en/latest/reference/commands/5tt2gmwmi.html).
- `tractography` estimates [responses](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2response.html) and [FODs](https://userdocs.mrtrix.org/en/latest/reference/commands/dwi2fod.html), then runs [ACT tracking](https://userdocs.mrtrix.org/en/latest/reference/commands/tckgen.html), [SIFT2](https://userdocs.mrtrix.org/en/latest/reference/commands/tcksift2.html), and [connectome construction](https://userdocs.mrtrix.org/en/latest/reference/commands/tck2connectome.html).

Outputs land in a folder per stage through one `{stage}` path default.

Image products share one type, `MRI<Kind,Space>`, and product names carry the processing state, so `raw_dwi` and `denoised_dwi` are both `MRI<DWI,Acquired>`. Type variables let one operation serve several products: `extract_b0` and `mean_b0` run on both acquired and corrected DWI, and `mrtransform` moves both T1w and tissue images. Label images use their own operation for nearest-neighbor resampling.

SPIT emits these command lines; it does not read acquisition metadata, check transforms, or judge image quality.

To try source discovery with empty placeholder files, run:

```sh
sh examples/commands/mrtrix3_act/mock_mrtrix3_inputs.sh
cargo run -- inputs examples/commands/mrtrix3_act/mrtrix3_act_discover.spitin --root examples/commands/mrtrix3_act/mrtrix3_mock_data -o examples/commands/mrtrix3_act/mrtrix3_mock_data/inputs.spitout
cargo run -- dag examples/commands/mrtrix3_act/mrtrix3_act.spit examples/commands/mrtrix3_act/mrtrix3_mock_data/inputs.spitout --root examples/commands/mrtrix3_act/mrtrix3_mock_data --paths
cargo run -- dag examples/commands/mrtrix3_act/mrtrix3_act.spit examples/commands/mrtrix3_act/mrtrix3_mock_data/inputs.spitout --root examples/commands/mrtrix3_act/mrtrix3_mock_data -o examples/commands/mrtrix3_act/mrtrix3_mock_data/jobs.spitdag
```

The script creates the three sessions and seven DWI runs listed in the example inventory. The recipe has no hand-written context or source records: it discovers session directories and scans the files. `inputs.spitout` nests 39 source identities under three session contexts, with no repeated paths because the pipeline declares them. The DAG contains 93 planned jobs. The files are empty, so the generated MRtrix3, FSL, and SynthSeg commands are for inspection only and cannot process this mock dataset.
