# Why store reports are missing

The dataset has sales for six stores: s01, s02, s03, s05, s07 and s09. Reports can be produced for **s01, s02 and s05**. Three stores cannot get a report, and because of them the chain summary (`reports/chain.pdf`) cannot be produced over all stores either.

| Store | What goes wrong | Root cause (where) |
| --- | --- | --- |
| **s03** | `store_report` for s03 fails its `@ min(2)`: "input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1". Its clean and price jobs would still work. | `data/sales/s03/` holds only `2026-W38.csv`. The `2026-W36.csv` and `2026-W37.csv` exports are missing, and the pipeline's report needs at least two weeks (`store_report ... @ min(2)`, `pipeline.spit` line 21). |
| **s07** | All three `price` jobs fail: "no `pricing` artifact for input `prices` of `price` at [store=s07,week=...]". So `report[store=s07]` fails too. | The price list exists, but it is named with an upper-case S: `data/pricing/S07.json`. The rule `path pricing: pricing/{store}.json` reads it as store `S07`, not `s07`. So SPIT sees an orphan `pricing[store=S07]` that has no sales, and no pricing for s07. The file must be renamed to `pricing/s07.json`. Store IDs are case-sensitive, and this breaks on a case-sensitive filesystem. |
| **s09** | All three `price` jobs fail: "no `pricing` artifact for input `prices` of `price` at [store=s09,week=...]". So `report[store=s09]` fails too. | There is no price list for s09: `data/pricing/s09.json` does not exist. |

Knock-on effect: `summary[]` (`reports/chain.pdf`) needs every store's report (`report @ vary(store)`), so it cannot be produced while s03, s07 and s09 are included. The planning run stopped at the first of these errors, the s07 price job, with `error: line 18, column 26: no pricing artifact ... at [store=s07,week=2026-W36]`. That hid the other two stores. `spit artifacts data/weekly.spitin` lists all of them.

Also noted, but not an error: store numbers s04, s06 and s08 have no files anywhere in `data/`. SPIT can't tell whether those stores exist, so nothing is reported for them. Check this if they are live stores.

How this was found: `spit dag data/weekly.spitin` (first error only), then `spit artifacts data/weekly.spitin` (full list), then comparing `data/sales/*` against `data/pricing/*`.

## This week's plan (Part 2)

`data/thisweek.spitin` is a new recipe for the same `pipeline.spit`. It leaves the problem stores out with two `skip` rules and does not touch any data file or command:

    skip sales count>=2 per [store]     # drops s03 (1 week), and the orphan S07 group (0 sales)
    skip pricing count=1 per [store]    # drops s07 and s09 (no pricing for the store)

`spit dag data/thisweek.spitin -o plan.spitdag` resolves 22 jobs:

- 9 `clean` jobs and 9 `price` jobs, for s01, s02 and s05 in weeks W36 to W38
- 3 `store_report` jobs
- 1 `chain_summary` over s01, s02 and s05
