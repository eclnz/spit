# Matching

Matching turns one call over product families into jobs over concrete artifacts. Compilation checks dimension compatibility; resolution checks actual matches in the inventory. Neither command arguments nor file contents influence matching.

## Syntax

```ebnf
selector = "@" ("where" "(" bindings ")"
               | "same" "(" dimensions ")"
               | "vary" "(" dimensions ")"
               | "each" "(" dimensions ")")
```

Each selector belongs to one call argument. `where` takes dimension/value bindings; the other selectors take dimension names. A selector must refer to a dimension of that input.

## Job contexts

The input with the most dimensions drives a normal operation and gives its outputs their dimensions, wherever it sits among the ports. Its observed artifacts determine the initial jobs: an input with `[config, seed]` creates only the config and seed pairs actually present, rather than every combination of known values. For an aggregation, `vary(run)` removes `run` from the output identity; `@ each(...)` adds a dimension, as described under selectors. You can write the output type and dimensions explicitly when helpful; SPIT checks them against the step and the [dimension order](pipeline.md#dimension-order):

```text
average : Image [subject, visit] = mean(processed @ vary(run))
```

A `one` input must resolve to exactly one artifact for each job, so every other input may only use dimensions the driving input has, unless it broadcasts them with `@ each(...)`; SPIT rejects a pipeline that breaks this before reading any inputs, and reports a missing match for a job. Every `many` input names the dimensions it collects at the call, with `@ vary(dimension, ...)`; the operation only says `many`, so one operation can collect runs in one step and sessions in another. Its command placeholder expands to one separately quoted argument per artifact, in natural order. Artifacts are compared dimension by dimension in the pipeline's [dimension order](pipeline.md#dimension-order). Within a value, runs of digits compare as numbers and other characters compare one by one, so `run=2` comes before `run=10`, ISO dates such as `2026-09-01` sort by date, and names sort by character (`lr-high`, `lr-low`, `warmup`). Values equal as numbers but written differently, such as `1` and `01`, are then ordered by their text. A many placeholder must occupy a whole argument. An operation takes at most one `many` input, which may sit beside `one` inputs; each of those is matched once per group:

```text
operation summarise(days: many Series @ min(2), policy: Policy) -> Summary
summary = summarise(reading @ vary(day), policy)
```

`@ min(2)` on the `many` input rejects a group with fewer than two artifacts; untyped, it is `days: many @ min(2)`. It goes beside the input it counts, not after the outputs: `-> Summary @ min(2)` is an error that gives the line rewritten.

One aggregate can remove several dimensions at once. List them in one `@ vary(...)`; their order within the clause does not change the collection order. With `summary [model, config]`, this makes one leaderboard over all model and config combinations, ordered first by model and then by config:

```text
operation leaderboard(summaries: many Summary) -> Table
board = leaderboard(summary @ vary(model, config))
```

`@ min(n)` counts the whole collection, across both dimensions. A call that writes two `@ vary` clauses is an error; put both dimensions in one clause. The collection order follows the pipeline's [dimension order](pipeline.md#dimension-order), even if `@ vary` lists those dimensions in another order.

Selectors narrow what an input matches:

```text
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
```

`where(revision=2)` keeps the artifacts with that value and takes `revision` out of matching, so a family with an extra dimension can join a less specific input. `same(station)` matches on `station` alone; the reference's other dimensions must then leave exactly one artifact for each job. Selectors can be combined, as in `frame @ where(acq=fast) @ vary(run)`.

`each` does the reverse of `vary`: it broadcasts an input over a dimension the driving input lacks, so the step runs once for every value and its outputs gain that dimension:

```text
dimensions [station, scenario]
source reading : Series [station]
source model : Model [scenario]
source parameters : Parameters [scenario]

forecast = predict(reading, model @ each(scenario), parameters)
```

With two stations and two scenarios, this makes four `forecast[station=...,scenario=...]` jobs. The values come from the artifacts of the broadcast input, so adding a scenario to the inputs adds its jobs. Other inputs are matched on the new dimension as usual; here `parameters` supplies the settings for each scenario. Only one input may broadcast a given dimension, and the driving input must not already have it. No source holds both `station` and `scenario`, so the `dimensions` line orders them, and `forecast` has dimensions `[station, scenario]`. `each` pairs with `vary`, so a sweep can be collected again. It crosses only the broadcast input's observed values with each driving artifact; values held by other inputs stay correlated through matching. For example:

```text
trial = simulate(reading, seed @ each(rep))
summary = average(trial @ vary(rep))
```
