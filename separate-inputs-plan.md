# Separate operation inputs (#103)

Complete in the accompanying implementation commit:
- Each distinct input product has its own arrow into a boxed multi-input operation.
- Shared and unrelated lines pass outside boxes with explicit crossings.
- Columns are reclaimed after operations so repeated shared-input joins stay narrow.
- Two/three-input, shared-line, component, MRtrix3, 1000-join and 10000-step tests passed.
- Release build, full release tests, clippy, answer keys and both benchmark comparisons passed.
- At 1000/2000/4000 shared-input joins, output stayed 42 columns wide and grew linearly in bytes.
- Push the update to PR #118. Local Messie remains blocked by its missing tokenizer cache.
