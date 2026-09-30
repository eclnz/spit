REPORT.md write refused by harness (subagents must return text). Key points from handback:
- 5/5 confidence; no spit errors at all. check passed first try.
- Design: discover sessions [sub,ses] from dirs + skip sessions count>=2 per [sub]; source paths in .spit.
- Guesses: path rules in .spit vs recipe (guide allows both, no preference); product and operation same name (`coreg`) - namespace not documented; recipe location relative to root.
- Gaps: exclusion only on stderr, not recorded in .spitdag/.spitout; ordering of leading-zero values (01 vs 1) undocumented; no product/operation namespace statement; no BIDS worked example.
- Errors: "found 19 source artifacts ... under `.`" - unclear what `.` refers to.
- Friction: output path rules repeat sub-/ses-/run- prefixes verbosely.
- vs Make/Snakemake: same time to first version, more trustworthy; cost is learning language + 3-step flow.
- Top 3: record excluded groups in .spitdag/.spitout; BIDS example; document ordering + namespace.
