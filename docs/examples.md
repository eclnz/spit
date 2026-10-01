# Examples

These walkthroughs contain everything needed to reproduce their plans in an empty directory. SPIT plans commands; the named tools do not need to be installed to inspect a plan. Save each code block under the filename above it, then run the shown commands with `spit` on your `PATH` (or replace `spit` with `cargo run --` from the repository). The larger example catalog follows the walkthroughs. For syntax rules, see the [language reference](language-reference.md).

## Ragged sweep: correlated seeds and collection order

Each configuration owns its seeds: `fast` has 1 and 2, while `deep` has only 1. The `seed[config,seed]` artifacts drive training. `model @ each(model)` broadcasts each model over those **observed** config/seed pairs; it does not manufacture `deep` seed 2. Declaring the summary's dimensions as `[model, config]` makes the final `many Summary` collection sort by model, then config. Without that explicit declaration, the derived product's dimensions are `[config, model]` and the final command receives config-first order.

Save as `sweep.spit`:

```spit
# Each config has its own seeds; every model is tried with every seed.
source model : Weights [model]
path model: models/{model}.pt
source config : Config [config]
path config: configs/{config}.yaml
source seed : Seed [config, seed]
path seed: seeds/{config}/{seed}.json
source testset : Data
path testset: eval/testset.parquet
operation train(model: Weights, config: Config, seed: Seed) -> Weights
command train: train --model {model} --config {config} --seed {seed} --out {output}
path trained: runs/{model}/{config}/{seed}/weights.pt
trained = train(model @ each(model), config, seed)
operation evaluate(weights: Weights, testset: Data) -> Metrics
command evaluate: evaluate {weights} {testset} --out {output}
path metrics: runs/{model}/{config}/{seed}/metrics.json
metrics = evaluate(trained, testset)
operation summarise(runs: many Metrics) -> Summary
command summarise: summarise {runs} --out {output}
path summary: summaries/{model}/{config}.json
summary : Summary [model, config] = summarise(metrics @ vary(seed))
operation leaderboard(summaries: many Summary) -> Table
command leaderboard: leaderboard {summaries} --out {output}
path board: leaderboard.csv
board = leaderboard(summary @ vary(model, config))
```

Save as `sweep.spitout`:

```text
sources:
    model[model=small]
    model[model=large]
    config[config=fast]
    config[config=deep]
    seed[config=fast,seed=1]
    seed[config=fast,seed=2]
    seed[config=deep,seed=1]
    testset
```

Run `spit dag sweep.spit sweep.spitout --commands` to see 17 jobs: six `train`, six `evaluate`, four `summarise`, and one `leaderboard`. For `config=deep`, there are two training jobs, one per model, both with seed 1. The final `leaderboard` command receives summaries in this order: `large/deep`, `large/fast`, `small/deep`, `small/fast`. Run `spit dag sweep.spit sweep.spitout -o sweep.spitdag` to save the plan. The input paths in this inventory are illustrative; add the named files under the paths declared above if you want SPIT to verify their existence with `--root`.

## Cohort: discovery, exclusion, and grouped removal

This recipe discovers session folders, removes a subject with fewer than two sessions, and excludes one damaged run while leaving its file in place. Each retained BOLD run gets motion correction and coregistration. A `many Bold` input collects runs per session; another collects session averages per subject.

Save as `cohort.spit`:

```spit
# BIDS sessions with a dropped subject and an excluded motion-corrupted run.
source t1w : T1 [sub, ses]
path t1w: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.nii.gz
source bold : Bold [sub, ses, run]
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz

operation motioncorr(bold: Bold) -> Bold
command motioncorr: motioncorr {bold} {output}
path mc: derivatives/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_mc.nii.gz
mc = motioncorr(bold)

operation bet(t1w: T1) -> T1
command bet: bet {t1w} {output}
path brain: derivatives/sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_brain.nii.gz
brain = bet(t1w)

operation coreg(bold: Bold, ref: T1) -> Bold
command coreg: coreg --ref {ref} --in {bold} --out {output}
path coregistered: derivatives/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_coreg.nii.gz
coregistered = coreg(mc, brain)

operation sessionavg(runs: many Bold) -> Bold
command sessionavg: sessionavg --out {output} {runs}
path avg: derivatives/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_avg.nii.gz
avg = sessionavg(coregistered @ vary(run))

operation longitudinal(sessions: many Bold) -> Bold
command longitudinal: longitudinal --out {output} {sessions}
path long: derivatives/sub-{sub}/sub-{sub}_long.nii.gz
long = longitudinal(avg @ vary(ses))
```

Save as `cohort.spitin`:

```spit
pipeline cohort.spit

discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
# Subject 03 has only one session and is removed as a whole.
drop [sub] where t1w count<2
exclude bold[sub=02,ses=02,run=2]    # motion spike
require t1w count=1 per [sub, ses]
require bold count>=1 per [sub, ses]
```

Create a tiny dataset beside those two files. These files can be empty because SPIT plans work without reading their contents:

```sh
for sub in 01 02 03; do
  for ses in 01 02; do
    if [ "$sub" = 03 ] && [ "$ses" = 02 ]; then continue; fi
    mkdir -p "sub-$sub/ses-$ses/anat" "sub-$sub/ses-$ses/func"
    touch "sub-$sub/ses-$ses/anat/sub-${sub}_ses-${ses}_T1w.nii.gz"
    for run in 1 2; do
      touch "sub-$sub/ses-$ses/func/sub-${sub}_ses-${ses}_task-rest_run-${run}_bold.nii.gz"
    done
  done
done
```

Run `spit inputs cohort.spitin` to see the four retained sessions and the removal records. Run `spit dag cohort.spitin --commands` to see 24 jobs: seven `motioncorr`, four `bet`, seven `coreg`, four `sessionavg`, and two `longitudinal`. No job reads `bold[sub=02,ses=02,run=2]` or an artifact of subject 03. The removed records remain visible in a saved `.spitdag`.

## Sensors: selectors, verification, and two outputs

A reading has station and day. Calibration has an extra revision, and reference has a measurement date. `where(revision=2)` chooses the approved calibration; `same(station)` matches the one reference for the station regardless of its date. `split_bands` writes two products in one job. `summarise` collects the low bands by day and matches one station policy beside that collection.

Save as `sensors.spit`:

```spit
# Readings from several stations, calibrated with a chosen calibration
# revision, compared with a station reference, split into two bands, and
# summarised per station under that station's policy.

path: derived/{product}/{entities}.csv

source reading : Series [station, day]
path reading: raw/{station}/{day}.csv
# Calibration files are kept for every revision; the pipeline picks one.
source calibration : Calibration [station, revision]
path calibration: calibration/{station}/r{revision}.json
# One reference per station, filed under the day it was measured.
source reference : Series [station, measured]
path reference: reference/{station}/{measured}.csv
source policy : Policy [station]
path policy: policy/{station}.toml

# Every station must have readings for days 1 and 2.

# The input with the most dimensions drives a step, whatever the port order.
# `where` pins the calibration revision, so it no longer takes part in matching.
operation calibrate(calibration: Calibration, series: Series) -> Series
verify calibrate: check_calibration {calibration} {series}
command calibrate: apply_calibration --calibration {calibration} {series} {output}
calibrated = calibrate(calibration @ where(revision=2), reading)

# `same(station)` matches the reference on station alone; each station must
# have exactly one, whatever day it was measured.
operation compare(series: Series, reference: Series) -> Series
command compare: subtract_reference {series} {reference} {output}
anomaly = compare(calibrated, reference @ same(station))

# One job writes both bands.
operation split_bands(series: Series) -> (low: Series, high: Series)
command split_bands: band_split {series} --low {low} --high {high}
low_band, high_band = split_bands(anomaly)

# A many input can sit beside single inputs, each matched once per group.
# The days arrive in natural order, and fewer than two is an error.
operation summarise(days: many Series, policy: Policy) -> Summary @ min(2)
command summarise: summarise --policy {policy} {days} --out {output}
path summary: derived/summary/{station}.json
summary = summarise(low_band @ vary(day), policy)
```

Save as `sensors.spitout`:

```text
sources:
    reading[station=north,day=1]
    reading[station=north,day=2]
    reading[station=north,day=10]
    reading[station=south,day=1]
    reading[station=south,day=2]
    calibration[station=north,revision=1]
    calibration[station=north,revision=2]
    calibration[station=south,revision=2]
    reference[station=north,measured=2024-03-01]
    reference[station=south,measured=2024-02-11]
    policy[station=north]
    policy[station=south]
```

Run `spit dag sensors.spit sensors.spitout --commands` to see 17 jobs: five each of `calibrate`, `compare`, and `split_bands`, then two `summarise` jobs. The `verify calibrate` command appears before each calibration command. The north summary takes days 1, 2, then 10; the unused north calibration revision 1 is reported separately. `Series`, `Calibration`, `Policy`, and `Summary` are types in operation signatures, while `reading`, `calibration`, `policy`, and `summary` are product names. Each call argument occupies the corresponding operation port and is type checked there.

## Stages: preprocessing and analysis

Stages group steps and can set their own output paths. This pipeline sorts three shards, merges the parts in each group, then tallies each merged result. Save as `stages.spit`:

```spit
# Text shards cleaned in one stage and summarised in the next.
path: {stage}/{product}/{entities}.txt

source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

stage preprocess:
    operation sort_lines(input: Lines) -> Lines
    command sort_lines: sort -u -o {output} {input}
    sorted = sort_lines(shard)

    operation merge(items: many Lines) -> Lines
    command merge: sort -m -u -o {output} {items}
    merged = merge(sorted @ vary(part))

stage analysis:
    path: results/{product}/{entities}.txt

    operation tally_lines(input: Lines) -> Tally
    command tally_lines: uniq -c {input} {output}
    tally = tally_lines(merged)
```

Save as `stages.spitout`:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

Run `spit dag stages.spit stages.spitout --paths` to see seven jobs: three `sort_lines`, two `merge`, and two `tally_lines`. The sorted and merged outputs use the `preprocess/` path default; the tallies use the `analysis` stage's `results/` override. `spit dag stages.spit stages.spitout -o stages.spitdag` records each job's stage for a backend.

## More example pipelines

Each pipeline below, under [`examples/`](../examples), checks cleanly and sits beside a `.spitin` recipe and a `.spitout` of its inputs. Recipes may add discovery, exclusion, drop, or require rules. Run the command from the repository root to see its jobs; add `--paths` to see each artifact's file or `-o plan.spitdag` to write them, or run `spit check` on the `.spit` or `.spitin` alone.

| Pipeline | Shows | Command | Jobs |
| --- | --- | --- | --- |
| [Branching](../examples/pipelines/branching.spit) | A shared policy, two branches with their own aggregations, and a recombination | `cargo run -- dag examples/pipelines/branching.spit examples/pipelines/branching.spitout` | 21 |
| [Observed groups](../examples/pipelines/rich_shapes.spit) | Several subjects and sessions, a reused reference, and two successive aggregations | `cargo run -- dag examples/pipelines/rich_shapes.spit examples/pipelines/rich_shapes.spitout` | 17 |
| [Nested aggregation](../examples/pipelines/complex.spit) | Partial types, irregular groups, and three successive aggregations | `cargo run -- dag examples/pipelines/complex.spit examples/pipelines/complex.spitout` | 25 |
| [Selectors](../examples/pipelines/selectors.spit) | `where`, `same`, a verification, a two-output step, and a many input beside a single input | `cargo run -- dag examples/pipelines/selectors.spit examples/pipelines/selectors.spitout` | 17 |
| [Archive revision](../examples/patterns/archive_revision/archive_revision.spit) | `where` selects the approved revision before joining calibration | `cargo run -- dag examples/patterns/archive_revision/archive_revision.spit examples/patterns/archive_revision/archive_revision.spitout` | 2 |
| [Per-group reference](../examples/patterns/per_group_reference/per_group_reference.spit) | `same(station)` finds one reference per station despite its measurement-date dimension | `cargo run -- dag examples/patterns/per_group_reference/per_group_reference.spit examples/patterns/per_group_reference/per_group_reference.spitout` | 3 |
| [Model fit](../examples/patterns/model_fit/model_fit.spit) | One `many` input, two outputs, `@ vary`, `@ min`, and `verify` | `cargo run -- dag examples/patterns/model_fit/model_fit.spit examples/patterns/model_fit/model_fit.spitout` | 2 |
| [Ragged sweep](../examples/patterns/ragged_sweep/ragged_sweep.spit) | `each(model)` broadcasts over per-config seeds, then `vary` collects the runs | `cargo run -- dag examples/patterns/ragged_sweep/ragged_sweep.spit examples/patterns/ragged_sweep/ragged_sweep.spitout` | 17 |
| [Cohort](../examples/patterns/cohort/cohort.spit) | BIDS sessions, a dropped subject, and an excluded run | `cargo run -- dag examples/patterns/cohort/cohort.spitin` | 24 |
| [Analytics](../examples/analytics/analytics.spit) | Five keyed joins, then day, customer, and tenant rollups | `cargo run -- dag examples/analytics/analytics.spit examples/analytics/analytics.spitout` | 34 |
| [Stages](../examples/stages/stages.spit) | Preprocessing and analysis stages with `{stage}` paths | `cargo run -- dag examples/stages/stages.spit examples/stages/stages.spitout` | 7 |
| [Nested stages](../examples/stages/nested.spit) | Stages within a stage | `cargo run -- dag examples/stages/nested.spit examples/stages/nested.spitout` | 9 |
| [Field survey](../examples/commands/field_survey/field_survey.spit) | Sidecar files, calibration, alignment between spaces, and commands | `cargo run -- dag examples/commands/field_survey/field_survey.spit examples/commands/field_survey/field_survey.spitout` | 93 |
| [MRtrix3 ACT](../examples/commands/mrtrix3_act/mrtrix3_act.spit) | A diffusion MRI pipeline in nested stages, from BIDS import to connectome | `cargo run -- dag examples/commands/mrtrix3_act/mrtrix3_act.spit examples/commands/mrtrix3_act/mrtrix3_act.spitout` | 93 |

The pattern examples each include a `.spitin` recipe and a `.spitout` inventory. The cohort recipe also has small placeholder source files, so its discovery, exclusion, and drop rules can be run directly. The `command_demo.spitin` recipe expects real shard files beside it; use its supplied `.spitout` to inspect the example jobs without creating a dataset.

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
