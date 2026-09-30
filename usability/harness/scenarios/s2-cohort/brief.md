# Task: resting-state fMRI cohort

A resting-state fMRI study is stored in BIDS layout under `data/`. Each subject (`sub-<sub>`) has one or more sessions (`ses-<ses>`). Each session has one T1w anatomical image (`anat/sub-<sub>_ses-<ses>_T1w.nii.gz`) and one or more resting BOLD runs (`func/sub-<sub>_ses-<ses>_task-rest_run-<run>_bold.nii.gz`). The JSON sidecars and top-level files are not inputs to anything.

Plan these steps:

1. Motion-correct every BOLD run: `motioncorr <bold> <out>` → `derivatives/sub-<sub>/ses-<ses>/func/sub-<sub>_ses-<ses>_run-<run>_mc.nii.gz`
2. Brain-extract each session's T1w: `bet <t1w> <out>` → `derivatives/sub-<sub>/ses-<ses>/anat/sub-<sub>_ses-<ses>_brain.nii.gz`
3. Coregister each motion-corrected run to the same session's brain: `coreg --ref <brain> --in <mc> --out <out>` → `derivatives/sub-<sub>/ses-<ses>/func/sub-<sub>_ses-<ses>_run-<run>_coreg.nii.gz`
4. Average each session's coregistered runs: `sessionavg --out <out> <coreg> <coreg> ...` with the runs in numeric order → `derivatives/sub-<sub>/ses-<ses>/func/sub-<sub>_ses-<ses>_avg.nii.gz`
5. For each subject, combine their session averages: `longitudinal --out <out> <avg> <avg> ...` with the sessions in order → `derivatives/sub-<sub>/sub-<sub>_long.nii.gz`

Study rule: a subject with fewer than two sessions is excluded from the study entirely, with no jobs at all, and the planning should tell us who was excluded. The values in output names are written exactly as in the input names (for example `sub-01`, `run-1`).
