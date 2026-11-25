# Runner responsibilities and static types

Track implementation under [issue 406](https://github.com/eclnz/spit/issues/406).

- [x] Lock before loading or repairing state. (3a8a692f)
- [x] Restore earlier outputs after failed setup and validate Slurm worker results. (3a8a692f)
- [x] Share force selection, job-key decoding, and review data. (3a8a692f)
- [x] Add the initial Python 3.10 mypy CI check and refresh the versioned fixtures. (3a8a692f)
- [ ] Type CLI commands, bound arguments, and scheduler outcomes, and expand the mypy check.
- [ ] Verify PR CI and run the Slurm scale and cluster checks before merging.
