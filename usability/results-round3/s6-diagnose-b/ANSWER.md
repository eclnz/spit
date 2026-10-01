# Stores whose reports cannot be produced

- **s03:** Only `sales/s03/2026-W38.csv` exists. The `store_report` operation needs revenue from at least two weeks (`@ min(2)`), but this store has only one.
- **s07:** Its three sales files have identity `store=s07`, but its price list is `pricing/S07.json`, which SPIT reads as `store=S07`. The case mismatch leaves no `pricing[store=s07]` for the `price` jobs, so no revenue or report can be made.
- **s09:** Its three sales files exist, but `pricing/s09.json` is absent. The `price` jobs have no pricing input, so no revenue or report can be made.

The chain summary also fails with the original recipe because it depends on all store reports, including these three. The weekly plan excludes these stores and includes the summary for s01, s02, and s05.
