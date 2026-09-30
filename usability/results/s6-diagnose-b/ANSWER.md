# Which store reports cannot be produced, and why

Found with `spit artifacts data/weekly.spitin` (`spit dag data/weekly.spitin` stops at the first one, s07).
Three of the six stores (s01, s02, s03, s05, s07, s09) cannot produce a report: s03, s07, s09. s01, s02 and s05 are fine.

## s03: only one week of sales (data problem)
- `data/sales/s03/` holds only `2026-W38.csv`. `2026-W36.csv` and `2026-W37.csv` are missing.
- `data/pipeline.spit` line 23, `store_report(weeks: many Revenue) -> Report @ drop(week) @ min(2)`, needs at least 2 weeks.
- spit: `input 'weeks' of 'store_report' needs at least 2 artifacts at [store=s03], found 1`.
- Fix: supply the missing weekly exports (W36 and/or W37) for s03.

## s07: price list exists under the wrong name (case mismatch in the file name)
- Sales for s07 are in `data/sales/s07/` (all 3 weeks), but the price list is `data/pricing/S07.json` (capital S).
- `data/pipeline.spit` line 6, `path pricing: pricing/{store}.json`, expects `pricing/s07.json`. Store ids are matched case-sensitively, so spit sees `pricing[store=S07]`, which matches no store's sales.
- spit: `no 'pricing' artifact for input 'prices' of 'price' at [store=s07,week=2026-W36]` (also W37, W38). So revenue for s07 cannot be made, nor `report[store=s07]`.
- Fix: rename `pricing/S07.json` to `pricing/s07.json` (not done here, data must not be touched).

## s09: no price list at all (missing file)
- `data/sales/s09/` has all 3 weeks, but there is no `data/pricing/s09.json`.
- Same `price` step error: `no 'pricing' artifact for input 'prices' of 'price' at [store=s09,week=...]` for all three weeks; so no revenue and no `report[store=s09]`.
- Fix: supply `pricing/s09.json`.

## Consequence for the chain summary
`summary` (`chain_summary(report @ vary(store))`) cannot be produced while s03, s07 and s09 are in the run, because it needs report for every store: `input 'reports' needs report[store=s03], which cannot be produced` (same for s07, s09).
Note `data/weekly.spitin` itself is valid and has no rules; it only names the pipeline, so nothing is skipped or required.

## Part 2
`plan.spitdag` covers s01, s02, s05 only (22 jobs: 9 clean, 9 price, 3 store_report, 1 chain_summary). It was written with
`bin/spit dag data/plan.spitin -o plan.spitdag`, where `data/plan.spitin` is my new recipe (the original data files are untouched):
`skip sales count>=2 per [store]` drops s03 (and the orphan pricing-only group S07), and `skip pricing count=1 per [store]` drops s07 and s09.
