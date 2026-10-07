# Cohort

This recipe discovers session folders, removes a subject with fewer than two sessions, and excludes one damaged run while leaving its file in place. Each retained BOLD run gets motion correction and coregistration. A `many Bold` input collects runs per session; another collects session averages per subject. One default path gives all five derived products BIDS-style names: optional groups omit the session or stage where a product has none, and `{@labels}` writes the dimensions each product has.

Save as `cohort.spit`:

```spit
# BIDS sessions with a dropped subject and an excluded motion-corrupted run.
path: derivatives/sub-{sub}[/ses-{ses}][/{@stage}]/{@labels}_{@product}
ext: .nii.gz

source t1w : T1 [sub, ses]
path t1w: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.nii.gz
source bold : Bold [sub, ses, run]
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz

operation motioncorr(bold: Bold) -> Bold
command motioncorr: motioncorr {bold} {@output}

operation bet(t1w: T1) -> T1
command bet: bet {t1w} {@output}

operation register(bold: Bold, ref: T1) -> Bold
command register: coreg --ref {ref} --in {bold} --out {@output}

operation sessionavg(runs: many Bold) -> Bold
command sessionavg: sessionavg --out {@output} {runs}

operation longitudinal(sessions: many Bold) -> Bold
command longitudinal: longitudinal --out {@output} {sessions}

stage anat:
    brain = bet(t1w)

stage func:
    mc = motioncorr(bold)
    coreg = register(mc, brain)
    avg = sessionavg(coreg @ vary(run))

long = longitudinal(avg @ vary(ses))
```

Save as `cohort.spitin`:

```spit
pipeline cohort.spit

discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
# Subject 03 has only one session and is removed as a whole.
exclude [sub] where t1w count<2
exclude bold[sub=02,ses=02,run=2]    # motion spike
require [sub, ses] where t1w count=1
require [sub, ses] where bold count>=1
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

Run `spit check cohort.spit --path-rules` to see the completed path for every product, including `derivatives/sub-{sub}/sub-{sub}_long.nii.gz` without a session or stage. `spit check cohort.spit --json` provides the same resolved templates as editor hints. Run `spit inputs cohort.spitin` to see the four retained sessions and the removal records. Run `spit dag cohort.spitin --commands` to see 24 jobs: seven `motioncorr`, four `bet`, seven `register`, four `sessionavg`, and two `longitudinal`. No job reads `bold[sub=02,ses=02,run=2]` or an artifact of subject 03. The removed records remain visible in a saved `.spitdag`.

Next: [Sensors](sensors.md).
