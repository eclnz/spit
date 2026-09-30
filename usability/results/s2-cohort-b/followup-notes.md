60 jobs. (1) sub-06: no change, ~1 min. (2) QC: 4 lines, first try, few minutes. (3) exclude run-3: longer than 1+2 combined; ended hand-editing a generated .spitout (rest-curated.spitout) and building from .spit + .spitout; recipe can no longer reproduce plan; re-running inputs silently brings run-3 back; exclusion invisible/unreported.
- `skip bold run=3 per [sub, ses]` passed check ("Recipe valid.") but did the opposite: dropped every session WITHOUT run 3, then cascaded to sub-02 via skip sessions -> "found 0 source artifacts and 0 contexts". No warning that everything was dropped / rule probably inverted.
- `run!=3` -> "invalid required dimension `run!`" - syntax-level, doesn't say inequality unsupported.
- where() only keeps; can't exclude.
- Suggests `exclude bold[sub=02,ses=02,run=3]` in .spitin.
- Found .spitout route only from guide Inputs section.
