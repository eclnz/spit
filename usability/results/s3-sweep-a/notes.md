REPORT refused. 5/5. One error: `source testset : TestSet` -> "expected product name followed by [dimensions]"; guessed `[]`, worked. Guide never shows dimensionless source; error doesn't suggest [].
Guesses: dimensionless input matches every job (undocumented); product can share name with dimension (`model @ each(model)`); pipeline-only recipe; executables not checked on PATH at plan time though guide says "must be available on PATH"; numeric seed ordering untested.
Gaps: dimensionless source; dimensionless input broadcast; minimal recipe; spitdag root absolute/relative.
Friction: artifacts list dims as [config, seed, model] (each-dim appended) - matters for {entities}.
vs: same as careful shell, faster than Snakemake (per-config seed sets need input functions).
Top3: document/accept `[]`/suggest it in error; document dimensionless match + minimal recipe; clarify PATH checking + root field.
