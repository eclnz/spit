# Designs and roadmap

The plans that answer the [usability findings](../FINDINGS.md), and the order to build them in.

| Plan | Resolves |
| --- | --- |
| [Removing inputs](removing-inputs.md) | F1, B1, F2, and with them F5, F7, F8 and B2 |
| [Showing each job's command line](commands-view.md) | F4 |
| [Messages that point at the cause](diagnostics.md) | B3, B5, F3, and the unmatched-file note for D5 |
| [Language changes](language.md) | F6, F9, F10 |
| [Small CLI and file fixes](small-fixes.md) | B4, B6, B7 |
| [Closing the guide's gaps](guide.md) | D1–D11 |

## Order

The work is in four phases: small independent fixes first, then the one large change, then what builds on it.

**Rules for every step.**

- One commit per step, on the `usability` branch.
- Each commit carries its tests and updates the guide for the behaviour it changes.
- Before each commit, run `cargo test` and `usability/harness/rebuild_keys.sh`.
- A key that stops resolving to the same jobs is either a regression, or an intended change that the commit explains and re-blesses.

### Phase 1: small, independent fixes (done)

These touch separate code and change no language, so they can land in any order.

1. **B7:** the help line ([small fixes](small-fixes.md#b7-the-help-line)). Done in `b782b87`.
2. **B6:** natural order for `external_inputs` ([small fixes](small-fixes.md#b6-order-a-spitdags-lists-as-many-inputs-are-ordered)). Done in `87780cf`.
3. **F4:** `dag --commands` ([commands view](commands-view.md)). Done in `3f27d75`.
4. **B3:** file names in every message ([diagnostics](diagnostics.md#b3-name-the-file-a-message-is-about)). Done in `8e17408`.
5. **B5, F3 part 1:** count and list unused sources ([diagnostics](diagnostics.md#b5-and-f3-say-what-the-inventory-holds-that-no-job-uses)). Done in `912df53`.
6. **D1–D4, D7, D9, D10:** guide sections for behaviour that is staying ([guide](guide.md#gaps-to-write-down-now)). Done in `13801f2`.

### Phase 2: removing inputs

One design built in three steps, each leaving the tool working. See [removing inputs](removing-inputs.md).

7. **`exclude`** in all three forms, inline and from CSV. Also in this step:
   - the fixed order of the input stage;
   - the `removed:` record in the `.spitout` and `.spitdag`;
   - removal notes on stderr;
   - the unknown-statement error (B2).

   `skip` still works during this step.
8. **`drop` replaces `skip`.** Also in this step:
   - `require` gains `count` comparisons;
   - rule order stops mattering;
   - removing every group becomes an error;
   - a `require` with no groups left becomes an error.

   Rewrite the Recipes section of the guide (closes D8).
9. **`dag --partial`,** the error that points to it and to `artifacts` (F5), and the hint that names an exclusion when an excluded input breaks a join.

### Phase 3: messages and language

10. **F3 part 2:** near-miss hints on a failed match and on unused sources. Also the unmatched-file note and `inputs --unmatched` (closes D5).
11. **F6:** sources without brackets, and bare names for products with no dimensions (closes D6).
12. **F9:** `@ vary(x, y)` with `@ drop(x, y)`.
13. **F10:** a call's `@ vary` inferred from the operation's `@ drop`.
14. **B4:** `check recipe.spitin --path-rules`, and the source wording on a pipeline.

### Phase 4: examples and follow-ups

15. **D11:** the five worked examples.
16. **F10:** named arguments in a call (optional).
17. **The VS Code extension** ([spit-vscode](https://github.com/eclnz/spit-vscode)): highlight `drop`, `exclude`, `where`, `has`, `missing` and `from`; drop `skip`. Add a `file` field to `check --json` diagnostics, so the editor can place a pipeline's error found while checking a recipe ([diagnostics](diagnostics.md#b3-name-the-file-a-message-is-about)).

## After the work: the second round

Re-run the [study](../README.md) with the same scenarios, rebuilt against the new guide. Then:

- Rewrite the s2 follow-up's answer key so it comes from a recipe with `exclude`, not an edited `.spitout`.
- Add a scenario that the old language could not express (scenario 3 with one leaderboard).
- Add one with vaguer briefs, to see how agents fail rather than whether they pass.

**Measures to compare with round 1:**

- `spit` calls and errors per run;
- how many runs parse the `.spitdag` by hand;
- time spent on the s2 follow-up's exclusion;
- the number of guide gaps raised.

If agents misread `drop`, fall back to the name `skip` with the new grammar, as agreed.
