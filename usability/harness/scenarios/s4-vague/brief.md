# Task: reports from an inherited sensor archive

The team wants an HTML report for each station in `data/`. The archive has daily readings under `raw/`, old calibration revisions under `calibration/`, one baseline per station under `baseline/`, and station metadata under `sites/`. Some stations miss days, and each baseline is filed under the date when it was recorded. Revision 3 is the approved calibration; other revisions remain in the archive but must not enter the plan.

The processing programs are fixed. Use these exact command forms and output locations:

- `calibrate --cal <calibration> --in <reading> --out <out>` writes `clean/<station>/<day>.csv` for each reading.
- `anomaly <clean> <baseline> <out>` writes `anomalies/<station>/<day>.csv`.
- `stationreport --site <site.toml> --out <out> <anomaly> ...` writes `reports/<station>.html`. Its anomaly inputs must be in day order.

Work out which archived files belong together from the dataset. The plan must adapt when another day or station appears without editing the pipeline.
