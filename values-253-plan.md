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

1. [ ] Language: `@scalar(...)` in command and `verify` templates; checked
   against the operation (a `one` input port that is a file); rejected in
   checks and redirections.
2. [ ] `.spitdag` version 13: a `{"scalar": P, "min": N, "max": N}` part;
   `dag --commands` shows it as `@scalar(P, min=N)`.
3. [ ] Runner: reads and checks each scalar before `verify`, logs it, fails
   the job naming the file and what it held; work identity and load checks.
4. [ ] Documentation: language reference, `docs/spitdag.md`, runner README.
5. [ ] spit-vscode: highlight `@scalar(...)`.
