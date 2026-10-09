# Multiline operation signatures (#104)

- [ ] Read operation signatures as logical declarations while preserving physical source locations.
- [ ] Test input/output lists, nested types, stages, composite bodies, errors and editor locations.
- [ ] Update and verify the language guide.
- [ ] Update spit-vscode on the same branch and test its grammar and extension.
- [ ] Run repository checks and performance comparisons; commit and link pull requests.
- [ ] Delete this plan in a dedicated final commit before merge.

Layout: continue while an operation's parentheses are open. Port lines indent beyond the declaration; a closing delimiter may align with it. The body begins only after the complete header ends in `:`. A later declaration is a recovery boundary.
