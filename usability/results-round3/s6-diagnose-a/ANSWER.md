# Stores whose reports cannot be produced

| Store | Root cause |
| --- | --- |
| `s03` | Only `data/sales/s03/2026-W38.csv` exists. The `store_report` operation requires at least two weekly revenue artifacts (`@ min(2)`), but only one week can be priced. |
| `s07` | The sales files use `store=s07`, but the pricing file is `data/pricing/S07.json`, which SPIT identifies as `store=S07`. The case mismatch means each `price` job for `s07` lacks its `pricing[store=s07]` input, so no weekly revenue or store report can be produced. |
| `s09` | Sales files exist for weeks W36–W38, but `data/pricing/s09.json` is absent. Each `price` job lacks its pricing input, so no weekly revenue or store report can be produced. |

The chain summary depends on all store reports, so the original full run is also incomplete. The plan excludes these three stores and the unused, mis-cased `S07` pricing artifact. It includes reports for `s01`, `s02`, and `s05`, then their chain summary.
