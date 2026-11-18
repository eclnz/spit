# Slurm safety fixes

- [x] Reserve array elements durably before submission and release them only with terminal evidence. Included in this commit.
- [x] Keep restart, cancellation, and polling within the reservation and call limits. Included in this commit.
- [x] Validate the effective submission arguments and keep scheduler operations on the same cluster. Included in this commit.
- [x] Save failure reports and clean up probes on interruption. Included in this commit.
- [ ] Run the release models, runner regressions, and a Docker Slurm check.

## Working tree verification

The state models, 11 release regressions, fast Slurm tests, and 49 site tests pass. Docker Slurm 23.11.4 passes the site checks, including the enforcement probes and cancellation of one held array element while its sibling stays pending. A five-job Docker run at `jobs = 1` leaves an empty queue and no outstanding reservations.

The fake-Slurm 100-task timeout and retry case passes. The initial 2,001-job case failed with insufficient diagnostics to establish why. A rerun with 16 fake workers passes at the same runner limit of 100 live tasks. The scale test includes failure lines and RPC counts in its output. A subsequent default-concurrency investigation has an incomplete log and no surviving test process. The original scale failure remains unresolved.

## Package adoption

- [x] Replace manual parsing of reservation, submission-history, and retry-history files with strict Pydantic schemas.
- [x] Preserve the existing JSON array shapes and verify records written before adoption, including zero memory and time requests.
- [x] Use the shared atomic writer for retry history and fail on unreadable or malformed records before submitting work.
- [x] Exercise production admission, reconciliation, result reads, rate limiting, and restart recovery through Hypothesis.
- [x] Add Hypothesis to the runner's test extra and the CI installation command.
- [x] Remove the isolated state-machine and SQLite experiment after evaluating its additional implementation and bookkeeping cost.
- [x] Run the complete runner suite, compatibility checks, and local Docker Slurm verification.

These implementation steps are in the working tree. The runtime schemas live beside the records they describe. Named tuples preserve the persisted arrays, and Pydantic owns their JSON validation and serialization. Retry writes use `intent.atomic`.

The generated test drives `Slurm._submit_free`, `Slurm._observe`, `Slurm._read_results`, and `Controller.start`. Its scheduler oracle tracks allocations independently. It injects refusal, a lost acknowledgement, interruptions before sending and after acceptance, missing or foreign queue rows, worker receipts before terminal evidence, restarts, and controller-host changes. The default is 100 examples of up to 40 steps. A separate run passes 300 examples of up to 80 steps.

Install the test dependencies and run the generated test from the repository root in a virtual environment.

```sh
python -m pip install -e './runner[test]'
python -m unittest discover -s runner/tests -p test_slurm_stateful.py -v
```

Pydantic supplies the runtime validation. Hypothesis supplies generated sequences and counterexample shrinking. The scheduler admission and identity rules remain explicit in the existing runner. Storage remains in the existing shared-filesystem format.

Validation passes 516 runner tests with two opt-in scale tests skipped, all eleven safety regressions, and the six record tests plus generated sequences on Python 3.13. Both deliberately introduced capacity-release faults are detected by the generated test's physical allocation invariant.

Docker Slurm completes the five-job lines workflow at `jobs = 1`. The memory probe runs exactly twice, ending as `OUT_OF_MEMORY` at 200 MB and `COMPLETED` at 400 MB. The final queue is empty and there are no outstanding dispatch files. The original default-concurrency scale investigation remains open.

A five-repeat local microbenchmark of reservation, debit, acknowledgement, restart reads, and terminal reconciliation gives medians of 0.974 ms per lifecycle before adoption and 0.979 ms after adoption. It excludes external scheduler latency.
