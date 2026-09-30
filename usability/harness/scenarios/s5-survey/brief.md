# Task: regional survey panel

Survey responses are under `data/responses/<region>/wave<N>.csv`. Waves are numbered 1, 2, 3, and so on, but not every region has every wave and the numbering is not always contiguous. Ignore backup files.

The build folder must be organised by phase (ingest, model, publish) exactly as below.

**Ingest.** Clean every response file: `clean_responses <raw> <out>` → `build/ingest/clean/<region>/wave<N>.csv`

**Model.** For each region, fit a panel model over all its cleaned waves in numeric wave order. One invocation writes two files: `fit_panel --coef <coef> --diag <diag> <wave> <wave> ...` → `build/model/coef/<region>.json` and `build/model/diag/<region>.txt`. `fit_panel` is expensive and crashes on malformed input, so before each fit, `validate_panel <wave> <wave> ...` (the same files, in the same order) must run as a check, and the fit must not run if the check fails.

**Publish.** A chart for each region: `plot_region <coef> <out>` → `build/publish/chart/<region>.svg`. One national table: `national_table --out <out> <coef> ...` in region-name order → `build/publish/national.csv`
