# Values read from files (eclnz/spit#253, in eclnz/spit#275)

A command passes a number a tool printed as an argument, without a shell
wrapper. The number stays in a file: an ordinary step writes it, as
`fslval {image} pixdim4 > {@output}`, and a later command reads it with
`@scalar(port, min=N, max=N)`. The runner reads the file when the job
starts, checks it holds one finite number within the bounds, and passes the
trimmed text as the argument, or as text within one.

Settled with the maintainer:

- Values are files, as eclnz/spit#265's `mean_gain -> Scalar .txt` is. Reruns,
  records and other runners need nothing new beyond a command part.
- The syntax is `@scalar(port, min=5)`, bounds beside the argument they guard.
- Numbers only. Text values would be a separate issue.

## Steps

1. [x] Language (`90846ea`): `@scalar(...)` in command and `verify` templates; checked
   against the operation (a `one` input port that is a file); rejected in
   checks and redirections.
2. [x] `.spitdag` version 13 (`90846ea`): a `{"scalar": P, "min": N, "max": N}` part;
   `dag --commands` shows it as `@scalar(P, min=N)`.
3. [x] Runner (`1a797cf`): reads and checks each scalar before `verify`, logs it, fails
   the job naming the file and what it held; work identity and load checks.
4. [x] Documentation: language reference, guide, `docs/spitdag.md`,
   binding page (`90846ea`); runner README and `examples/runnable/numbers`
   (`1a797cf`).
5. [x] spit-vscode: highlight `@scalar(...)` (`c2e2076` in spit-vscode).
6. [ ] Delete this plan before the branch merges; #265 builds on this.
