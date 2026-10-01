# Stores whose reports cannot be produced

| Store | Root cause | Where it fails |
| --- | --- | --- |
| `s03` | Only `sales/s03/2026-W38.csv` exists. Its single revenue artifact is below the `store_report` operation's `@ min(2)` requirement. | `data/pipeline.spit`, `store_report` declaration and `report = store_report(...)`; a second weekly sales export is needed. |
| `s07` | Sales files identify the store as `s07`, but its pricing file is `pricing/S07.json`. SPIT reads that as `pricing[store=S07]`, so the `price` jobs for `s07` cannot find `pricing[store=s07]`. | `data/pricing/S07.json` conflicts in letter case with `data/sales/s07/`; all three `revenue[store=s07,...]` artifacts fail. |
| `s09` | Sales exports exist for three weeks, but `pricing/s09.json` is absent. | The `price` operation cannot find a `pricing[store=s09]` source for any week, so no `s09` revenue or report can be produced. |

The original `chain_summary` also fails because it needs those three incomplete reports. The plan excludes all three affected stores and summarizes `s01`, `s02`, and `s05`.
