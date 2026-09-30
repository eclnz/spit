# Compiler stress pipelines

These are compile-only fixtures. They model artifact identities and types; the
`.dat` and `.bin` paths are illustrative, and no commands are supplied to run
the jobs. Use `check`, `check --path-rules`, or `dag` to inspect them.

```sh
cargo run -- check examples/stress/type_lab.spit
cargo run -- dag examples/stress/type_lab.spit examples/stress/type_lab.spitout
cargo run -- dag examples/stress/observatory.spit examples/stress/observatory.spitout
cargo test --test stress_pipelines
```

| Pipeline | Jobs | What it tests |
| --- | ---: | --- |
| [Type lab](type_lab.spit) | 129 | Deeply nested types, four local type variables in one signature, repeated generic operations, inference through `Unknown` products, partial unknowns, cross-port constraints, keyed fusion, and successive rollups. |
| [Observatory](observatory.spit) | 153 | Six source dimensions, uneven input groups, global and scoped inputs, several branches that reunite, six levels of aggregation, and one generic `evidence` operation used at device, site, and organization scope. |

In the type lab, `project` has the signature
`Stream<Frame<$Kind,$SourceSpace>,$Processing> × Calibration<$Kind,$SourceSpace,$TargetSpace> → Stream<Frame<$Kind,$TargetSpace>,$Processing>`.
For the lidar branch it infers
`Stream<Frame<Lidar,World>,Normalized>`; for the camera branch it infers
`Stream<Frame<Camera,World>,Normalized>`. The unclassified branch starts with
an unknown frame kind, which the camera calibration resolves. The opaque
branch retains an unknown processing state in
`Classified<Camera,World,Unknown>`.

The tests also mutate the valid type lab to confirm that compilation rejects
a camera calibration on a lidar stream, a second lidar stream in the camera
port, and a declared Mars output after a World calibration. A missing camera
slice confirms that keyed matching does not silently borrow another capture.
They also check that a type mismatch inferred across two stages fails with an
empty inventory, before any concrete jobs are created.
