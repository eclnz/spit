# Task: weekly server log reports

Our ops team keeps one log per server per day under `data/logs/<server>/<YYYY-MM-DD>.log`. Plan the weekly log report pipeline:

1. For every daily log, make a digest: `logdigest --in <log> --out <digest>`. Digests go to `digests/<server>/<date>.json`.
2. For each server, roll up all its digests into one weekly report: `rollup --out <report> <digest> <digest> ...` with the digests in date order. Reports go to `reports/<server>/week.json`.
3. Combine every server's weekly report into one fleet report: `fleetsum <report> <report> ... -o <fleet report>` with the reports in server-name order. It goes to `reports/fleet.json`.

Some servers miss a day now and then; use whichever days exist. The logs folder also holds rotated and compressed files and a notes file that are not daily logs: ignore them. Servers come and go, so the pipeline must not list servers or dates itself.
