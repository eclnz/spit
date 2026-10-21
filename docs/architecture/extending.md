# Extending SPIT

A new feature enters at the stage that owns its meaning. It lowers into forms that already exist, when they keep that meaning: sources, path rules, operations, bindings and jobs.

- **A source declared `beside` another** stays an ordinary source, with a path derived from the main source. It keeps only the group its diagnostics need (`Pipeline::sidecar_groups`).
- **An operation carried out by steps** lowers to those steps. It keeps only the provenance that its views and diagnostics need (`Pipeline::calls`, `Invocation::origin`; see [parsing](parsing.md#calls-to-operations-carried-out-by-steps)).
- **A feature whose meaning differs** gets its own model, with a narrow boundary to the shared DAG, rather than flags threaded through unrelated stages. A `check`, for example, is a `CheckDef` that becomes one bound command per checked artifact of a job, and changes no job, path or dependency (see [binding](binding.md)).

A feature that needs branches in many later stages is a sign to revisit its design.

## Before building one

For a feature that changes the syntax, the `.spitdag`, or what a runner must do, write down:

- **the stage that owns it,** and the [architecture page](../architecture.md) whose contract it changes;
- **what it lowers to,** or why it needs a new form;
- **the invariants it keeps:**
    - dependencies stay acyclic;
    - output paths stay unique;
    - binding stays deterministic;
    - `Unknown` types stay honest;
- **how it meets the rest of the language:** selectors, imports, stages, paths, partial plans and diagnostics.

Test each of those meetings, and malformed input, not only the example that motivated the feature.

## Identities

Keep identities typed or interned from parsing to diagnostics, and write them as text only for a message. A display string reused as a key is the smell to look for (see [performance](performance.md)).

## Repositories that follow

A change to what the editor reads, or to the `.spitdag`, needs a matching change in spit-vscode or spit-bash. AGENTS.md says when, and how the pull requests are linked.
