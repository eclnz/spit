# Store reports that cannot be produced

- **s03:** Only `data/sales/s03/2026-W38.csv` exists for this store. The `store_report` operation in `data/pipeline.spit` requires at least two weekly revenue artifacts (`@ min(2)`), so its one week cannot make `reports/s03.pdf`.
- **s07:** Sales files exist for weeks W36–W38, but its pricing file is `data/pricing/S07.json`. SPIT reads that as `pricing[store=S07]`, while the sales use `store=s07`. The case mismatch leaves every `price` job for s07 without a matching pricing artifact, so `reports/s07.pdf` cannot be made.
- **s09:** Sales files exist for weeks W36–W38, but there is no `data/pricing/s09.json`. Every `price` job for s09 lacks its pricing input, so `reports/s09.pdf` cannot be made.

The original chain summary also cannot be produced because it depends on all three unavailable reports. `remaining.spitin` excludes these stores and the orphan uppercase `S07` pricing identity. `plan.spitdag` contains the three good store reports (s01, s02, s05) and their chain summary.
