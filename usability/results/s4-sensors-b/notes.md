REPORT.md write refused by harness. Key points from handback:
- 4/5; no errors or warnings at all.
- Longest thought: how to say "only revision 3" / "one baseline per station with differing date" -> found where/same in Selectors; no worked archive-version example.
- Guesses: pipeline-only recipe legal (guide shows only recipes with rules); `rev{revision}` placeholder after literal; where value string vs number; ISO date ordering in many inputs (guide only says numbers compare as numbers); op/product/program same name; same(station) ignoring extra dim `recorded` not clearly stated; absolute root in spitdag.
- Gaps: minimal recipe / scan-only route not documented; archive/version selection pattern; where/same value matching string vs number; string ordering in many; unused source artifacts (rev1/2) silent - fine but undocumented.
- Messages: "17 source files verified" vs "found 20 source artifacts" count difference unexplained.
- Friction: operation + command lines duplicate one declaration; default path rule easy to forget.
- vs Snakemake: same speed, more confident; Snakemake needs input functions for per-station baseline and variable days.
- Top 3: worked example for archive-version + differently-named-per-group; document value matching/ordering; document minimal recipe + explain verified vs found counts.
