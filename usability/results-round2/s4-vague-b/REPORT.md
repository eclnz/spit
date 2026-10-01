# Report

## 1. Outcome

Yes. `stations.spit`, `stations.spitin`, `stations.spitout`, and `plan.spitdag` are present. Confidence: **5/5**. The DAG has 19 jobs: eight calibrations, eight anomaly jobs, and three reports. The command preview matches the requested arguments and paths, including the two East days in order. The DAG reports no left-out jobs.

## 2. Walkthrough

1. Read `TASK.md`, the file list, `GUIDE.md`, `REPORT.md`, and `spit help`. The file list revealed eight readings, three stations, one dated baseline per station, and calibration revisions other than 3.
2. Read the guide's operation and selector sections. Wrote source path rules for the archive and output path rules for the three derived products. Used `where(revision=3)`, `same(station)`, and `vary(day)`.
3. Ran `spit check` on the recipe. It reported `type \`calibration\` must start with a capital letter`, `type \`baseline\` must start with a capital letter`, and `type \`site\` must start with a capital letter`. I had mistaken product names for type names in port declarations. I gave the sources capitalized types and updated the ports; the next check passed and showed all seven explicit path rules.
4. Ran `spit inputs` with `--root data` and saved `stations.spitout`. It found 20 archived source artifacts.
5. Ran `spit dag --commands` to inspect every expanded command. It resolved 19 jobs and reported three unused calibration artifacts, which are the unapproved revisions. The commands used revision 3, each station's own baseline, and day-ordered anomaly inputs.
6. Ran `spit dag ... -o plan.spitdag`. It again resolved 19 jobs. I parsed the JSON to confirm the job counts, dataset root, and zero `left_out` entries.

## 3. Stuck points

The baseline date is unrelated to each reading date, so I spent the most thought on joining it without accidentally matching days. `baseline @ same(station)` addressed that, since the archive has exactly one baseline per station.

## 4. Guesses

I inferred that `rev{revision}.json` would read `rev3.json` as `revision=3`, and that `same(station)` would discard `recorded` for matching. Both were supported by the successful input scan and expanded commands. I initially guessed lowercase product names were valid port types; the compiler corrected that.

## 5. Guide gaps

In **Operations and commands** and **Selectors**, I wanted one complete example combining a filtered revision, a reference with an unrelated date, and a per-group `many` input. The separate examples explained each feature, but I had to assemble and verify their interaction. The linked example files were unavailable as the task warned.

## 6. Error messages

The three `type ... must start with a capital letter` messages were precise and gave line and column positions. They did not suggest that the lowercase words were product names, but the cause became clear from the type section. The `3 source artifacts are used by no job (calibration: 3)` note was helpful evidence that unapproved revisions were excluded, though I still checked the commands to identify which ones were used.

## 7. Language friction

The baseline file has a date dimension that must be preserved in its path but ignored for matching. Expressing that required `same(station)` on the call, while the approved calibration required `where(revision=3)`. The syntax works, but these distinct selectors took time to distinguish. I also had to define types or omit them for ports instead of referring to source products as lowercase types.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was slower to learn for this small initial archive because I had to understand selectors and port typing. Once compiled, it produced all 19 jobs and their dependencies without enumerating dates or stations. The same pipeline should handle new readings and stations if they have the required revision 3 calibration, baseline, and site file.

## 9. Top three changes

1. Add a complete guide example that joins an irregular daily source to a single dated reference per group, filters revisions, and aggregates each group's available days.
2. In the type error, mention that a lowercase product name in a port position is being parsed as a type, with an untyped-port or capitalized-type example.
3. Show a compact preview in `dag` that summarizes which source values a `where` selector kept and skipped, making an approved-revision check quicker.
