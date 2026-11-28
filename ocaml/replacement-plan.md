# OCaml compiler replacement

Draft PR target: `dev`. Replace the executable and build without introducing DAG v19. Documentation replacement is excluded at the user's request.

## Checkpoints

- [x] Isolate the OCaml implementation with version-18 output: `d0da9010`.
- [x] Record the bounded black-box verification and its limitations: `bc4ae9e4`.
- [x] Remove the Rust compiler, tests and Cargo configuration; retain contract fixtures and runnable example acceptance tests.
- [x] Build and install a standalone native OCaml executable; retain bytecode builds and test both.
- [x] Move version ownership to VERSION and migrate the release bump tool.
- [x] Switch CI, formatting hooks and build helpers to OCaml.
- [ ] Complete compiler parity: [#422](https://github.com/eclnz/spit/issues/422).
- [ ] Coordinate editor protocol, syntax, companion PR and breaking version bump: [#423](https://github.com/eclnz/spit/issues/423).

The executable, removal and build-migration checkpoints landed in `ec567af5`. The feature remains in progress: retain this plan until the merge preparation described in AGENTS.md.

## Verification boundary

Native and bytecode parser suites, 94 compiler cases and CLI output checks pass. A separately built reference executable passes the paired v18 planned-work, ordering, invalid-input, execution and reuse fixture. Provenance differs; that fixture is not replacement certification.

The full runner run with the OCaml executable completed 649 tests: 35 failures, 44 errors and two skips. Compiler-dependent example, incremental and review integrations fail on missing recipe/CLI support. The example harness reports 71 failures including uncovered libraries/probes; all nine usability scenarios fail to compile. Staged installation, execution outside the checkout, coordinated version-file migration, format/lint checks, strict documentation build and Messie pass. The Docker cluster installer has only a shell syntax check; its container build was not run.

Existing example and usability tests exercise features outside the implemented OCaml subset and remain merge blockers under #422. Keep them intact. The existing profiling workloads likewise require legacy syntax and editor/discovery commands; they cannot certify performance parity yet. Native/bytecode scale measurements are smoke checks, not a completed cross-compiler benchmark.

The combined experiment remains on `codex/ocaml-dag-prototype`; DAG v19 remains on `codex/dag-artifact-table`. Neither is part of this replacement's format contract.

## Recipe loading checkpoint

The CLI accepts a recipe for dag with JSON and file output. A recipe names
its pipeline, root, output directory and source patterns under inputs.
Paths are relative to their declaring file. Source discovery matches
literal text and dimension placeholders in source paths, then uses the
existing resolver and DAG writer. Duplicate roots and conflicting source
rules are errors.

Recipe selection rules, tools, authored contexts, optional path groups,
wildcards and shaped placeholders remain under #422, along with the
existing example pipelines' syntax. Recipe loading alone does not satisfy
the full replacement acceptance suite.
