# Matching

The input with the most dimensions normally drives a step. SPIT makes a job for each observed driving artifact and looks up one artifact for each other ordinary input at the matching dimension values. It reports missing or ambiguous matches instead of silently choosing a file.

## Join on shared dimensions

```spit
source reading : Series [station, day]
source policy : Policy [station]
operation calibrate(series: Series, policy: Policy) -> Series
calibrated = calibrate(reading, policy)
```

Each `reading[station=...,day=...]` drives a job. That job reads the `policy` for the same station. A source with no dimensions matches every job. The [language reference](../manual/operations.md#operations-and-commands) describes the exact matching rules.

## Choose and relax a match

```spit
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
```

`where(revision=2)` first selects that revision and removes `revision` from matching. `same(station)` matches only on `station`; if two references match, the job is ambiguous. These selectors can be combined with collections, such as `frame @ where(acq=fast) @ vary(run)`.

## Collect with `many` and `@ vary`

```spit
operation summarise(days: many Series @ min(2)) -> Report
report = summarise(calibrated @ vary(day))
```

`@ vary(day)` collects the input's days for each remaining identity, such as each station, and removes `day` from the output identity. `many` names the operation port's cardinality; `@ vary` names the dimension the call collects. `@ min(2)` rejects a collection smaller than two. A collection is sorted by the pipeline's dimension order, with numeric runs ordered naturally (`2` before `10`). One operation can have at most one `many` input, beside ordinary inputs that are matched once per group. `@ vary(model, config)` collects over both dimensions in one call.

## Broadcast with `@ each`

```spit
dimensions [station, scenario]
source reading : Series [station]
source model : Model [scenario]
operation predict(series: Series, model: Model) -> Series
forecast = predict(reading, model @ each(scenario))
```

For each observed station reading, `@ each(scenario)` adds one job per observed scenario model. Its output gains `scenario`. This does not manufacture missing pairs *inside* the driving input: a ragged `[config, seed]` source still drives only its observed pairs. A later `@ vary(scenario)` can collect the forecasts again. When sources do not order `station` and `scenario` together, the `dimensions` line sets their order.

For complete runnable examples, see the [sensor walkthrough](sensors.md) and [ragged sweep](sweep.md). The [reference](../manual/operations.md#operations-and-commands) covers selectors, cardinality, output inference, and all restrictions.

Next: [Paths](paths.md).
