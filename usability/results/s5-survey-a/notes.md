REPORT refused. 5/5; no errors. Biggest difficulty: `dag --paths` shows files but not command lines or verify commands - had to write .spitdag and read JSON to check argv.
Guesses: minimal pipeline-only recipe; path matching anchored (wave3.csv.bak ignored) - undocumented; multi-output + @drop combined never shown; path product: rules inside stage never shown; region alpha order.
Odd: `spit help` first line says "...and write a script" but nothing writes a script; zero-dim product printed as `national[]`; external_inputs in spitdag sorted lexically (wave=10 before wave=2) while commands numeric.
Friction: stage default path not usable since each product layout differs - explicit per-product rules.
vs: same as shell, faster than Make/Snakemake (numeric wave order free; glob order bug avoided; verify direct).
Top3: dag option to print expanded command + verify lines; document anchored matching + minimal recipe; worked example combining many + multi-output + drop + verify.
