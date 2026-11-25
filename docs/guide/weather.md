# The weather family

The weather family is four versions of one small analysis of a weather network: sites in regions, with a temperature file for each day. Each version adds to the one before, and together they use every feature of the language, so every page of this guide can point to a concrete case. They are in [`examples/runnable/weather/`](https://github.com/eclnz/spit/tree/dev/examples/runnable/weather), they need only `awk`, `sort`, `grep` and Python, and each one runs with `spit-bash`. Each version has a folder of its own, and `shared/` holds the library, script and tools file that more than one uses.

## The versions

- [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit) finds each site's climate, the mean of its daily readings, and it names its own dataset, so it needs no recipe.
- [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit) removes each sensor's calibration offset first, and compares each site with its region. It adds types, stages, checks, a `verify`, resources, and a library of shared definitions.
- [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit) asks what each site's mean would be under each warmer or cooler scenario. It adds broadcasting, an operation carried out by steps, files that travel beside other files, folders, including one that starts as a copy of another and a directory view, standard streams, tools and the keyword blocks.
- [v4_survey.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spit) reads an untidy survey, with its recipe [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin). It adds the recipe's rules to exclude and require data, discovery by directory, and a person's review.

The first three read the tidy dataset in `network/`, and the fourth reads `survey/`, which has missing days, a site that has not reported, a stray file and a file of verdicts. Each version writes to its own folder under `output/`.

<!-- spit-doc: fragment -->

```sh
cd examples/runnable/weather
spit dag v3_scenarios.spitin --counts
spit-bash run v3_scenarios.spitin -j 4
```

Two inventories show the `.spitout` form: [v1_two_sites.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_two_sites.spitout) is written by hand, in the nested form, and [v4_survey.spitout](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitout) is what `spit inputs` writes for the survey, with its removals and verdicts.

## How the features are checked

[features.json](https://github.com/eclnz/spit/blob/dev/examples/checks/features.json) lists each feature of the language with a pattern that a version must match and the section of the guide that teaches it. `examples/checks/check.py` fails when a feature is missing from its file, when a SPIT keyword belongs to no feature, or when a guide section named there does not exist. A feature that the language gains therefore has to gain a case here before it is merged.
