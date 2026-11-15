# A weather network

This page reads one real analysis from start to finish, with the commands to run it. The dataset is a made-up network of 16 weather sites in three regions. The pipeline cleans each site's year of readings, compares each site with its region, counts frost and heat days, finds the sensors that need a visit, and draws a figure. It has 98 jobs, runs in a few seconds on a laptop, and needs only Python with `numpy` and `matplotlib`.

The analysis lives in [`weather/`](https://github.com/eclnz/spit-showcase/tree/main/weather) of the [spit-showcase](https://github.com/eclnz/spit-showcase) repository, beside six more on public data: spoken digits, MRI quality control, SARS-CoV-2 variant calling, elevation tiles and machine-learning models. Later pages quote them too.

## Get the data

Install SPIT and `spit-bash` as in [First run](first-run.md) first. Then get the showcase:

<!-- spit-doc: fragment -->

```sh
git clone https://github.com/eclnz/spit-showcase
cd spit-showcase/weather
unzip -q weather-data.zip -d data
python3 -m pip install numpy matplotlib
```

The zip holds 1,487 files. Every site has a file that says where it stands, a file for its sensor's calibration, and an hourly temperature file for every fourth day of 2025:

```text
data/raw/terrain.csv
data/raw/alerts/frost.json
data/raw/alerts/heat.json
data/raw/coast/dune/site.json
data/raw/coast/dune/calibration.json
data/raw/coast/dune/2025-01-01.csv
data/raw/coast/dune/2025-01-05.csv
...
```

The regions are `coast`, `highland` and `valley`. The two alert files hold the thresholds that define a frost day and a heat day.

## Declare what exists

A pipeline starts with the families of files it reads. Each family is a `source`, with a type, the kind of file, and the dimensions that tell its files apart:

```spit
source location : Site .json [region, site] @check(json)
source terrain : Terrain .csv
source calibration : Calibration .json [region, site] @check(json)
source reading : Readings<Raw> .csv [region, site, day] @check(day_of_hours)
source threshold : Threshold .json [alert] @check(json)
dimensions [region, site, day, alert]

default path: [{region}/][{site}/]{@product}[_{alert}][_{day}]
default ext: .csv

check nonempty: test -s {@path}
check json: python3 -m json.tool {@path}
check day_of_hours: sh -c 'test "$(wc -l < "$1")" -eq 25' sh {@path}
default check: nonempty
```

- `reading[region=coast,site=dune,day=2025-06-26]` is one day's file. `terrain` has no dimensions, so it is one file that every job can use.
- `Readings<Raw>` is a type with an argument. Later the cleaned series is `Readings<Clean>`, so a step cannot mistake one for the other.
- `dimensions [region, site, day, alert]` sets the order of the dimensions, because no source holds `alert` beside the others.
- One `default path:` places every product the pipeline makes. The bracketed parts appear only when the product has that dimension, so `cleaned` is `coast/dune/cleaned.csv`, `regional` is `coast/regional.csv` and `league` is `league.csv`. See [paths](artifacts.md#paths).
- Checks are tests the runner applies to a file. `day_of_hours` demands a header and 24 rows, and `default check: nonempty` tests every file the pipeline makes. See [checks](operations.md#artifact-checks).

## Clean each site's year

The first stage turns each site's 92 day files into one series:

<!-- spit-doc: setup
source calibration : Calibration .json [region, site]
source reading : Readings<Raw> .csv [region, site, day]
default path: [{region}/][{site}/]{@product}[_{day}]
default ext: .csv
-->

```spit
stage clean:
    operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>
    command clean: python3 {@here}/steps/clean.py --calibration {calibration} --days {days.day} -o {@output} {days}
    cleaned = clean(reading @vary(day), calibration)
```

`reading @vary(day)` gathers all of a site's days into one collection, so there is one `clean` job for each site, 16 in all, and its output keeps `region` and `site`. The `calibration` has the same `region` and `site`, so each job reads its own site's file. `@min(2)` refuses a site with fewer than two days. In the command, `{days}` is one argument for each day file in date order, `{days.day}` is the matching list of dates, and `{@here}` is the folder of the pipeline, where the scripts are. See [collections](pipelines.md#collections).

## Compare each site with its region

The second stage uses one operation for three different groupings:

<!-- spit-doc: setup
source location : Site .json [region, site]
source cleaned : Readings<Clean> [region, site]
default path: [{region}/][{site}/]{@product}
default ext: .csv
-->

```spit
stage compare:
    operation average(series: many Readings<S>) -> Readings<S>
    command average: python3 {@here}/steps/average.py -o {@output} {series}
    regional = average(cleaned @vary(site))
    overall = average(cleaned @vary(region, site))

    operation reduce(series: Readings<Clean>, location: Site) -> Readings<SeaLevel>
    command reduce: python3 {@here}/steps/reduce.py --location {location} -o {@output} {series}
    reduced = reduce(cleaned, location)
    regional_reduced = average(reduced @vary(site))

    operation compare(readings: Readings<SeaLevel>, baseline: Readings<SeaLevel>) -> Anomaly
    command compare: python3 {@here}/steps/compare.py {readings} {baseline} {@output}
    anomaly = compare(reduced, regional_reduced)

    operation rank(anomalies: many Anomaly) -> Table<League>
    command rank: python3 {@here}/steps/rank.py --regions {anomalies.region} --sites {anomalies.site} -o {@output} {anomalies}
    league = rank(anomaly @vary(region, site))
```

- `average` takes the readings of any kind, because `S` is a type variable, and passes the kind through to its output. `@vary(site)` averages the sites of each region, and `@vary(region, site)` averages the whole network.
- `compare(reduced, regional_reduced)` pairs each site with its own region. `reduced` has `[region, site]` and `regional_reduced` only `[region]`, so each site job reads the one baseline whose region matches. See [matching](pipelines.md#matching).
- The types keep the comparison honest. A site's series reduced to sea level may only be compared with another reduced series. Passing `regional`, the region's actual temperatures, is caught before anything runs:

```text
error: line 50, column 32: type mismatch at `compare.baseline`: product `regional` is Readings<Clean>, expected Readings<SeaLevel>
```

## Count the alerts

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
source threshold : Threshold .json [alert]
dimensions [region, site, alert]
default path: [{region}/][{site}/]{@product}[_{alert}]
default ext: .csv
-->

```spit
stage alerts:
    operation tally(series: Readings<Clean>, threshold: Threshold) -> Count
    command tally: python3 {@here}/steps/tally.py --alert {alert} --threshold {threshold} -o {@output} {series}
    alert_days = tally(cleaned, threshold @each(alert))
```

`threshold @each(alert)` runs every site against every alert file. With 16 sites and 2 alerts the stage has 32 jobs, and a third alert file would add 16 more with no change to the pipeline. `{alert}` in the command is the job's alert, as text. See [broadcasting](pipelines.md#broadcasting).

## Draw the figure

The last stage tabulates what the figure needs and calls an operation that is carried out by steps, one for each panel:

<!-- spit-doc: fragment -->

```spit
use figure from figure.spit as fig

stage plot:
    figure = fig::figure(
        terrain,
        location @vary(region, site),
        reading @where(region=coast, site=dune, day=2025-06-26),
        calibration @where(region=coast, site=dune),
        cleaned @where(region=coast, site=dune),
        region_table,
        day_table,
        alert_table,
        anomaly @vary(region, site),
        league
    )
```

`use figure from figure.spit as fig` imports one operation from a library file. `@where(...)` picks the single site and day that panel **b** draws. The call expands into seven jobs, six panels and one that lays them out, and the plan shows them under the call. See [composite operations](pipelines.md#composite-operations) and [imports](pipelines.md#imports).

## Say which dataset

The pipeline is for any weather network. The recipe, `weather.spitin`, says where this one is and what to leave out:

<!-- spit-doc: recipe
--- weather.spit
source location : Site .json [region, site]
source terrain : Terrain .csv
source calibration : Calibration .json [region, site]
source reading : Readings<Raw> .csv [region, site, day]
source threshold : Threshold .json [alert]
dimensions [region, site, day, alert]
default path: [{region}/][{site}/]{@product}[_{alert}][_{day}]
default ext: .csv
-->

```spit
pipeline weather.spit
root data/raw
output data/results

inputs:
    location: {region}/{site}/site.json
    terrain: terrain.csv
    calibration: {region}/{site}/calibration.json
    reading: {region}/{site}/{day}.csv
    threshold: alerts/{alert}.json

exclude reading[region=valley,site=sedge,day=2025-04-15]    # buried in cuttings
exclude reading[region=valley,site=sedge,day=2025-04-19]    # buried in cuttings
require [region, site] where calibration count=1
require [region, site] where location count=1
```

The `inputs:` block gives each source's path, and `output` sends everything the jobs write to `data/results`. The two `exclude` lines leave out two days at one site, with the reason beside each. The `require` lines stop the plan if any site lacks its calibration or its location. See [datasets](datasets.md).

## Plan it

<!-- spit-doc: fragment -->

```sh
spit dag weather.spitin --counts
```

```text
note: excluded reading[region=valley,site=sedge,day=2025-04-15] (line 14): buried in cuttings
note: excluded reading[region=valley,site=sedge,day=2025-04-19] (line 15): buried in cuttings
note: found 1485 source artifacts under `data/raw`
note: 98 jobs resolved: 16 in clean, 40 in compare, 32 in alerts, 10 in plot.
jobs  step                             stage
  16  cleaned = clean                  clean
   3  regional = average               compare
   1  overall = average                compare
  16  reduced = reduce                 compare
   3  regional_reduced = average       compare
  16  anomaly = compare                compare
   1  league = rank                    compare
  32  alert_days = tally               alerts
   1  region_table = tabulate_regions  plot
   1  day_table = tabulate_days        plot
   1  alert_table = tabulate_alerts    plot
      figure = fig::figure             plot
   1    figure::a = fig::map           plot
   1    figure::b = fig::cleaning      plot
   1    figure::c = fig::means         plot
   1    figure::d = fig::anomalies     plot
   1    figure::e = fig::alert_days    plot
   1    figure::f = fig::drift         plot
   1    figure = fig::compose          plot
   7    in this call
  98  total
```

The counts match the story: 16 sites make 16 `clean` jobs, one for each of three regions makes 3 regional averages, and 16 sites by 2 alerts make 32 tallies. `spit dag weather.spit --tree` draws how the stages connect, and `--commands` lists every command before anything runs. See [Plans](plans.md).

## Run it

<!-- spit-doc: fragment -->

```sh
spit-bash run weather.spitin -j 8
```

```text
98 done, 0 checked, 0 failed, 0 current, 0 blocked
```

The run takes about six seconds with eight cores. `data/results/figure.png` is the finished figure, and the folder beside it holds every product, laid out by the path rule. Run the same command again and every job is current, in under half a second.

## Change something

The point of a plan is that a change reruns only what it touches. Try three.

**Move a threshold.** Edit `data/raw/alerts/frost.json` and plan again:

```text
since the last run: 1 threshold changed (alert=frost)
16 jobs can start, 3 jobs may run if an upstream output changes, 79 jobs are current
16 jobs will replace existing outputs:
  tally x16: input changed
```

Only the 16 frost tallies read that file. Running it reruns them and the three jobs that read their results, which is 19 jobs, and the other 79 stay current.

**Take out an exclusion.** Delete the two `exclude` lines from the recipe:

```text
since the last run: clean: command; the artifacts it reads changed (1 job)
1 job can start, 21 jobs may run if an upstream output changes, 76 jobs are current
```

The only job whose inputs changed is `clean` for the one site, so it runs. The 21 jobs below it run only if its output comes out different. If it does not, they are skipped.

**Break a file.** Truncate `data/raw/coast/dune/2025-06-26.csv` and run:

```text
[3] failed  clean region=coast site=dune: check day_of_hours on input days coast/dune/2025-06-26.csv failed: exited with status 1
[3] held up 21 other jobs
0 done, 0 checked, 1 failed, 21 not run (an upstream job failed), 76 current, 0 blocked
```

The check on the source caught it before `clean` ran. The 21 jobs that depend on the site's series did not run, the other 76 stayed current, and the run exits with status 1. Restore the file and nothing needs to run.

## Where to go next

Each page of the guide goes deeper into one part of what you just read:

- [Artifacts](artifacts.md) for dimensions, types and paths.
- [Operations](operations.md) for commands, checks and resources.
- [Pipelines](pipelines.md) for matching, collections and broadcasting.
- [Datasets](datasets.md) for recipes, exclusions and reviews.
- [Plans](plans.md) for finding out why a job is missing.
- [Runs](runs.md) for rerun decisions and a run's files.

The other showcase analyses use the same parts differently: [`digits`](https://github.com/eclnz/spit-showcase/tree/main/digits) joins takes into speaker tables, [`anatomical`](https://github.com/eclnz/spit-showcase/tree/main/anatomical) lets through only subjects that pass a quality check, [`covid`](https://github.com/eclnz/spit-showcase/tree/main/covid) writes index files beside their data, and [`ml`](https://github.com/eclnz/spit-showcase/tree/main/ml) crosses every model with every dataset.
