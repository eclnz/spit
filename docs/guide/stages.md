# Stages

Stages group steps and can set their own output paths. This pipeline sorts three shards, merges the parts in each group, then tallies each merged result. Save as `stages.spit`:

```spit
# Text shards cleaned in one stage and summarised in the next.
path: {@stage}/{@product}/{@entities}.txt

source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

stage preprocess:
    operation sort_lines(input: Lines) -> Lines
    command sort_lines: sort -u -o {@output} {input}
    sorted = sort_lines(shard)

    operation merge(items: many Lines) -> Lines
    command merge: sort -m -u -o {@output} {items}
    merged = merge(sorted @ vary(part))

stage analysis:
    path: results/{@product}/{@entities}.txt

    operation tally_lines(input: Lines) -> Tally
    command tally_lines: uniq -c {input} {@output}
    tally = tally_lines(merged)
```

Save as `stages.spitout`:

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

Run `spit dag stages.spit stages.spitout --paths` to see seven jobs: three `sort_lines`, two `merge`, and two `tally_lines`. The sorted and merged outputs use the `preprocess/` path default; the tallies use the `analysis` stage's `results/` override. `spit dag stages.spit stages.spitout -o stages.spitdag` records each job's stage for a backend.

Next: [Reusable steps](reusable-steps.md).
