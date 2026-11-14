# Many inputs and rerun dependencies

Tracking issues: eclnz/spit#278 and eclnz/spit#279.
Base: dev at b59fd2a67e871f1f1e6454a9963bf81e897cdb45.
Companion: spit-vscode, branch codex/many-inputs-and-reruns.

## Design

- Command functions follow the landed @scalar convention: @join(port, "sep")
  and @list(port). Ordinary {port} expansion is unchanged. Joins keep paths
  structured through the DAG so the runner can check effective roots. Empty
  separators and empty collections are supported. Dimension joins produce text.
- Lists are runner-owned temporary UTF-8 files holding absolute paths, one per
  line. They are shared through verify and command sequences and cleaned up on
  success, failure and cancellation. Their names do not identify work.
- A fixed source ending in here is read relative to the declaring file, including
  imported libraries. It is an ordinary content-tracked input under the separate
  pipeline_folder root. It has no dimensions, folder form or beside output.
- Anchored is an operation-only mark, following the later proposal on #221.
  Absolute input/output paths identify anchored work, and relocation is refused.
- Recipe or pipeline tools from selects a .spitenv; dag --tools overrides it.
  Saved inputs retain the environment file reference; saved DAGs embed the
  declarations. A probe's exact stdout identifies its tool, and failure prevents
  jobs from starting. Used probes run once per load, from the dataset root,
  without an implicit shell and with a 30-second timeout.
- Jobs depend on every tool unless uses narrows their operation. Tool names are
  declared before uses. The first positive name selects a set; a leading !name
  starts from all tools. Later names add and !name removes. No stage/default
  uses declarations are included. Dependency selection happens once per step.
- accept-tools updates only tool identity evidence for recorded jobs; it retains
  file stamps/hashes, check evidence and output paths, and refuses other changed
  work before writing any record.
- The shared DAG contract is version 15. Screens remain scheduled for version 16.
  SPIT, runner and extension target the existing development release 0.5.0.

## Work

- [x] Review both trackers, all five child issues, proposals and landed work.
- [x] Preserve the October documentation checkout; restore and verify an isolated
  current-dev source snapshot through the connector because git fetch is denied.
- [x] Implement collection parsing, validation, binding, structured wire parts,
  runner rendering/lifetime, diagnostics and readable command display.
- [x] Implement fixed local sources, imported paths, anchored work/relocation,
  tool declarations/probes, per-operation dependencies and explicit acceptance.
- [x] Add compiler, runner and extension regression tests and a runnable example.
- [x] Update language, format, architecture, runner, examples and extension docs.
- [x] Run the Rust release suite, clippy, fmt, answer keys and example checks.
- [x] Verify stored output changes: only version 14 -> 15, empty top-level/job
  tools and false anchored fields changed in the two existing JSON snapshots.
- [x] Run all 360 runner tests under a temporary child-reaping supervisor.
- [x] Run all 97 extension diagnostics/grammar tests, including the new file kind
  and located diagnostics against the final compiler.
- [x] Run pipeline and dataset performance regression checks against dev.
  Initial comparison: pipeline check 19.8 -> 20.4 ms; dataset dag 183.8 ->
  194.1 ms. Both checks pass the repository's 1.3x threshold.
- [x] Prepare linked draft PRs targeting dev, preserving real upstream ancestry.
  The user explicitly authorized publishing the implementation, tests and
  documentation on 2026-11-14. Both repositories use the same branch name.
- [ ] Complete strict site/Mermaid and Messie checks when dependencies are available.
- [ ] Before merge, delete this plan in a dedicated final commit with the
  recovery command in its message, as AGENTS.md requires.

## Validation limits

Git fetch and package downloads are denied by the environment proxy. Source
snapshots were restored through the GitHub connector and every blob was checked
against its Git SHA. Draft commits must use the real upstream parent, not the
local snapshot-import commit.

The strict site build was attempted offline and online. Its pinned tzdata wheel
is unavailable locally and the proxy denies the download. Changed Mermaid
rendering has therefore not been checked. Messie was attempted with the installed
executable; it cannot load its WordLlama embedding model/tokenizer and downloads
are disabled. Neither check is claimed to pass.

Sandboxed Python thread wakeups stall the runner's cutoff test. The outer runner
passes it. The container's PID 1 also leaves killed grandchildren as zombies,
causing the cancellation test to fail identically on unchanged dev. A temporary
supervisor using Linux child-subreaper support reaps them; the full 360-test suite
passes under it without changing repository tests or runner cancellation code.

## Benchmark output

```text
 steps  build      check     --json   --hovers
  2000  old        19.8      20.4      67.5
  2000  new        20.4      21.7      48.1

no time is more than 1.3x the old build's
subjects extra  build        inputs       dag     check
    1000    30  old           41.7    183.8      1.9
    1000    30  new           45.2    194.1      1.8
    1000    30  new+ext       41.0    190.7      1.9

no time is more than 1.3x the old build's
```

Final regression comparison:

```text
 steps  build      check     --json   --hovers
  2000  old        20.5      20.8      47.3
  2000  new        19.5      20.0      48.1

no time is more than 1.3x the old build's
subjects extra  build        inputs       dag     check
    1000    30  old           44.3    184.8      1.7
    1000    30  new           44.4    193.1      1.8
    1000    30  new+ext       43.0    191.3      1.7

no time is more than 1.3x the old build's
```
