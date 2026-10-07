# Sensors

A reading has station and day. Calibration has an extra revision, and reference has a measurement date. `where(revision=2)` chooses the approved calibration; `same(station)` matches the one reference for the station regardless of its date. `split_bands` writes two products in one job. `summarise` collects the low bands by day and matches one station policy beside that collection.

Save as `sensors.spit`:

```spit
# Readings from several stations, calibrated with a chosen calibration
# revision, compared with a station reference, split into two bands, and
# summarised per station under that station's policy.

path: derived/{@product}/{@entities}.csv

source reading : Series [station, day]
path reading: raw/{station}/{day}.csv
# Calibration files are kept for every revision; the pipeline picks one.
source calibration : Calibration [station, revision]
path calibration: calibration/{station}/r{revision}.json
# One reference per station, filed under the day it was measured.
source reference : Series [station, measured]
path reference: reference/{station}/{measured}.csv
source policy : Policy [station]
path policy: policy/{station}.toml

# Every station must have readings for days 1 and 2.

# The input with the most dimensions drives a step, whatever the port order.
# `where` pins the calibration revision, so it no longer takes part in matching.
operation calibrate(calibration: Calibration, series: Series) -> Series
verify calibrate: check_calibration {calibration} {series}
command calibrate: apply_calibration --calibration {calibration} {series} {@output}
calibrated = calibrate(calibration @ where(revision=2), reading)

# `same(station)` matches the reference on station alone; each station must
# have exactly one, whatever day it was measured.
operation compare(series: Series, reference: Series) -> Series
command compare: subtract_reference {series} {reference} {@output}
anomaly = compare(calibrated, reference @ same(station))

# One job writes both bands.
operation split_bands(series: Series) -> (low: Series, high: Series)
command split_bands: band_split {series} --low {low} --high {high}
low_band, high_band = split_bands(anomaly)

# A many input can sit beside single inputs, each matched once per group.
# The days arrive in natural order, and fewer than two is an error.
operation summarise(days: many Series @ min(2), policy: Policy) -> Summary
command summarise: summarise --policy {policy} {days} --out {@output}
path summary: derived/summary/{station}.json
summary = summarise(low_band @ vary(day), policy)
```

Save as `sensors.spitout`:

```text
sources:
    reading[station=north,day=1]
    reading[station=north,day=2]
    reading[station=north,day=10]
    reading[station=south,day=1]
    reading[station=south,day=2]
    calibration[station=north,revision=1]
    calibration[station=north,revision=2]
    calibration[station=south,revision=2]
    reference[station=north,measured=2024-03-01]
    reference[station=south,measured=2024-02-11]
    policy[station=north]
    policy[station=south]
```

Run `spit dag sensors.spit sensors.spitout --commands` to see 17 jobs: five each of `calibrate`, `compare`, and `split_bands`, then two `summarise` jobs. The `verify calibrate` command appears before each calibration command. The north summary takes days 1, 2, then 10; the unused north calibration revision 1 is reported separately. `Series`, `Calibration`, `Policy`, and `Summary` are types in operation signatures, while `reading`, `calibration`, `policy`, and `summary` are product names. Each call argument occupies the corresponding operation port and is type checked there.

Next: [Stages](stages.md).
