REPORT.md write refused by harness. Key points from handback:
- 5/5; no errors; all checks passed first try. Used --root data with recipe in working folder.
- Gaps: --root on dag is documented as only a file-existence check, but with a recipe it's also the scan root / path base; no example of variable inside filename (rev{rev}.json); spitdag root absolute and undocumented; selectors silently drop filtered sources.
- Messages: "found 20 source artifacts" vs "17 source files verified" unexplained -> suggest naming unused files and the selector that excluded them.
- Guesses: port order vs placeholder order independence; ISO date sort.
- vs Snakemake: faster than Snakemake (needs input functions for baseline + rev3); cost is reading a long guide.
- Top 3: report found-but-unused sources + excluding selector; document --root as scan root + variable-in-filename example; relative or documented spitdag root.
