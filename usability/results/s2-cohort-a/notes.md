REPORT refused. 5/5, no errors. Recipe placed in data/ with `pipeline ../rest.spit` (guide doesn't say .. allowed) so root = data. discover sessions + skip sessions count>=2 per [sub] + two require sanity rules + source path rules in recipe.
Biggest difficulty: what paths are relative to / where recipe should live.
Messages: `check rest.spit --path-rules` lists `bold (source): MISSING` because rules are in the recipe - alarming; skip warning is labelled warning for intended behaviour and doesn't say why (1 session, needs >=2).
Guesses: `..` in pipeline line; avoided same name op/product (unsure of namespace); skip per [sub] drops bold with extra run dim; typed many ports since unsure how to write untyped many.
Gaps: recipe/data layout guidance; .spitdag JSON schema undocumented (command-line format, targets, executables, fingerprint); skipped groups not persisted; untyped many syntax; op/product namespace; --path-rules MISSING for recipe-supplied rules.
vs: same speed as careful shell, more confidence; most time learning 470-line guide; Snakemake would need exclusion + per-subject session lists by hand.
Top3: persist skipped groups with reason in .spitout/.spitdag; document .spitdag schema; layout worked example + fix --path-rules MISSING wording.
