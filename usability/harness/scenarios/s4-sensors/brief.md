# Task: station calibration reports

Environmental monitoring data is under `data/`:

- `raw/<station>/<YYYY-MM-DD>.csv`: daily readings. A station may miss a day.
- `calibration/<station>/rev<N>.json`: calibration files. The archive keeps every revision, but only revision 3, the currently approved one, may be used.
- `baseline/<station>/<date>.csv`: each station's single reference baseline, filed under the date it was recorded. The dates differ from station to station.
- `sites/<station>/site.toml`: station metadata.

Plan these steps:

1. Calibrate every daily reading with its station's revision-3 calibration: `calibrate --cal <calibration> --in <reading> --out <out>` → `clean/<station>/<day>.csv`
2. Compare each calibrated reading with its station's baseline: `anomaly <clean> <baseline> <out>` → `anomalies/<station>/<day>.csv`
3. One report per station from its site metadata and all its anomaly files: `stationreport --site <site.toml> --out <out> <anomaly> ...` in day order → `reports/<station>.html`
