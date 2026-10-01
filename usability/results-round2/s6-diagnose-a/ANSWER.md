# Stores whose reports cannot be produced

- **s03:** Only `sales/s03/2026-W38.csv` exists. The `store_report` operation in `data/pipeline.spit` requires at least two weekly revenue artifacts (`@ min(2)`), so its one available week cannot make `reports/s03.pdf`.
- **s07:** Its three sales files exist, but the price list is `data/pricing/S07.json`. The `pricing` source reads that as `store=S07`, while the sales and `price` jobs need `store=s07`. The case mismatch prevents all three revenue artifacts and `reports/s07.pdf`.
- **s09:** Three sales files exist, but `data/pricing/s09.json` is absent. Each `price` job lacks its pricing input, so `reports/s09.pdf` cannot be produced.

The original chain summary also depends on these incomplete reports. The plan in `plan.spitdag` excludes these three stores and summarizes s01, s02, and s05.
