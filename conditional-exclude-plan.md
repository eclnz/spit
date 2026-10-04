# Conditional exclude (#76)

- [x] Parse conditional `exclude` through the existing coverage rule path; reject `drop` with a migration message (20d30e1).
- [x] Preserve removal order, records, and reasons; update diagnostics and hover words (20d30e1).
- [x] Convert tests, fixtures, examples, and documentation; verify behavior and stored output (20d30e1).
- [x] Update the VS Code extension grammar and tests on the same branch name (`spit-vscode` f3189bc).
- [x] Run required checks and performance comparisons, then commit both repositories and link pull requests (20d30e1, `spit-vscode` f3189bc; [SPIT #82](https://github.com/eclnz/spit/pull/82), [spit-vscode #22](https://github.com/eclnz/spit-vscode/pull/22)).

The `.spitdag` removal record fields and meaning remain the same; only the rule text changes. The rendered output confirms spit-bash needs no update.
