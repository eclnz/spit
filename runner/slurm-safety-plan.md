# Slurm safety fixes

- [x] Reserve array elements durably before submission and release them only with terminal evidence. Included in this commit.
- [x] Keep restart, cancellation, and polling within the reservation and call limits. Included in this commit.
- [x] Validate the effective submission arguments and keep scheduler operations on the same cluster. Included in this commit.
- [x] Save failure reports and clean up probes on interruption. Included in this commit.
- [ ] Run the release models, runner regressions, and a Docker Slurm check.

## Working tree verification

The state models, 11 release regressions, fast Slurm tests, and 49 site tests pass. Docker Slurm 23.11.4 passes the site checks, including the enforcement probes and cancellation of one held array element while its sibling stays pending. A five-job Docker run at `jobs = 1` leaves an empty queue and no outstanding reservations.

The fake-Slurm 100-task timeout and retry case passes. The initial 2,001-job case failed with insufficient diagnostics to establish why. A rerun with 16 fake workers passes at the same runner limit of 100 live tasks. The scale test includes failure lines and RPC counts in its output. A subsequent default-concurrency investigation has an incomplete log and no surviving test process. The original scale failure remains unresolved.
