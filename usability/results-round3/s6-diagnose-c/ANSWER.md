# Stores whose reports cannot be produced

- **s03:** Only `data/sales/s03/2026-W38.csv` exists for this store. The `store_report` step in `data/pipeline.spit` requires at least two weekly revenue artifacts (`@ min(2)`), but it can produce only one.
- **s07:** Its sales are identified as `store=s07`, but its pricing file is `data/pricing/S07.json`, identified as `store=S07`. SPIT matches store values by case, so the `price` step has no `pricing[store=s07]` input for any of its three weeks.
- **s09:** It has three sales files but no `data/pricing/s09.json`. The `price` step therefore cannot produce weekly revenue for this store.

The chain summary also cannot be produced from the original recipe because it requires all store reports, including these three incomplete ones.
