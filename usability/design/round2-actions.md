# Actions from the second usability study

This plan follows the [second study](../ROUND2.md). All 18 plans matched their keys, so the goal is to shorten the detours participants reported and make the next study able to test the documentation fairly. Keep domain walkthroughs in [`docs/examples.md`](../../docs/examples.md); keep [`docs/language-reference.md`](../../docs/language-reference.md) focused on language rules and short syntax examples. Named arguments remain out of scope: calls keep positional inputs, with each slot checked against its operation port.

Work through these items in order. Each implementation step gets its own commit and the checks required by the [roadmap](README.md#checks-before-every-commit). Record any change to an answer key and its reason.

## 1. Make the worked examples self-contained (done)

**Evidence:** Sweep, cohort, sensor, and survey participants asked for complete examples. `docs/examples.md` currently links to example files but does not itself contain their pipelines, recipes, inventories, and results. The trial guide did not include the linked files.

**Change:** Expand `docs/examples.md` with a small set of complete, runnable walkthroughs based on the existing executable examples. Start with a ragged sweep, a cohort recipe, and a sensor or selector pipeline; add a compact stages, multiple outputs, and verification walkthrough if those features cannot fit clearly in the first three. Each walkthrough should give the full pipeline and recipe or inventory needed to reproduce it, a minimal source layout, the command to run, expected job count, and a few representative planned commands or outputs. For the sweep, show which input drives jobs, how `@ each` retains correlated config and seed values, and how declared output dimensions determine the order of a later `many` collection. For the cohort, show discovery, `exclude`, `drop`, and a `many` input together. Keep the larger domain examples linked as a catalog after these walkthroughs.

**Done when:** A reader can copy each walkthrough into an empty directory and obtain the documented jobs without opening another repository file. The examples check and plan successfully, and their stated counts and ordering are verified against the binary. Links to the full example fixtures still resolve in the repository.

## 2. Clarify the rules exposed by the study (done)

**Evidence:** Two sweep participants invented a nonexistent seed combination before the missing-input error corrected them. Both vague-brief sensor participants used lowercase product names where operation types belong.

**Change:** In `docs/language-reference.md`, explain the general rule for the driving input, `@ each` correlation versus a cross product, positional input type checking, the distinction between types and products, and how output dimension order affects `many` ordering. Use short syntax fragments there and link to the complete walkthroughs in `docs/examples.md`. Check the README's short guide for contradictory or missing wording.

**Done when:** The reference answers those rule questions without copying domain walkthroughs into it; every link to a walkthrough lands on the relevant section. The sweep and sensor walkthroughs demonstrate the rules with plans produced by SPIT.

## 3. Deliver every local guide reference in trial packages (done)

**Evidence:** `usability/harness/build_guide.py` includes the language reference but omits `docs/examples.md` and `docs/spitdag.md`, while the generated guide links to them. Participants repeatedly looked for those files.

**Change:** Update the guide builder and `make_run.sh` so a trial contains the self-contained examples and `.spitdag` reference. Choose either a single `GUIDE.md` with working internal anchors or bundled documentation files with valid relative links; keep linked example fixtures only where a reader needs to execute a larger catalog example. Add a package check that finds broken local links and anchors, and verify the trial still excludes answer keys and repository source.

**Done when:** In a freshly built trial directory, every local link in the guide resolves within that directory, the worked examples are readable offline, and no answer key is present.

## 4. Tighten the two cheap CLI messages (done)

**Evidence:** A vague sensor run hit the lowercase-type error; another run hit the `--commands` and `-o` conflict. Both recovered, but the repair required an extra call.

**Change:** Add a contextual hint when a lowercase product name appears in an operation type position, showing the expected type/product distinction and the offending name. State in CLI help and the README that `dag --commands` cannot be combined with `-o`; make the conflict error say which command to run for each output. Keep the existing type checks and positional port order.

**Done when:** Focused tests cover the message and option conflict; a user can correct either call from the error or help alone. Existing answer keys resolve to the same jobs.

## 5. Investigate plan-inspection requests with the repaired guide (done)

**Evidence:** Participants requested per-operation job counts, a preview of output and collection order, and more detail about unmatched or unused files. Both diagnosis participants correctly found the three broken stores but spent time on the uppercase orphan file. Existing `inputs --unmatched`, unused-source notes, `--paths`, and `dag --commands` may already provide part of this information.

**Change:** First document and try the existing inspection commands in the relevant walkthrough. Then run short, focused trials on the sweep and diagnosis tasks with the repaired guide. If participants still need them, design a concise per-operation count view and a source-match or collection-order preview. For the orphan case, assess whether the unmatched-file output should identify the close lowercase group and suggest separate exclusion of the uppercase source. Add output only for a demonstrated gap, with an example of its exact text and a test for its ordering.

**Done when:** Each request is either answered by a discoverable existing command or has a specific CLI design backed by a repeat observation. Any implemented output is deterministic and does not obscure existing failure messages.

## 6. Repeat the affected usability tasks (done)

After items 1–4, rerun `s3-sweep`, `s3-one-board`, `s4-vague`, and `s6-diagnose` in isolated trials; include the cohort follow-up to check the complete recipe example. Use the same model and tasks for replicates. Retain command counts, failed calls, artifacts, reports, and participant tool transcripts so guide use and sandbox access can be audited. Compare the specific detours above, not only whether the answer keys pass. Use those results to decide item 5 before adding a broader CLI view.

The [third study](../ROUND3.md) and its [inspection design](round3-inspection.md) record the results. The trial host did not provide participant tool transcripts, so the access audit remains unverified; wrapper call logs, plans, and available reports were retained.
