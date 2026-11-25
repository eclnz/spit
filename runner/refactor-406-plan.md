# Runner responsibilities and static types

Track implementation under [issue 406](https://github.com/eclnz/spit/issues/406).

- [x] Lock before loading or repairing state. (3a8a692f)
- [x] Restore earlier outputs after failed setup and validate Slurm worker results. (3a8a692f)
- [x] Share force selection, job-key decoding, and review data. (3a8a692f)
- [x] Add the initial Python 3.10 mypy CI check and refresh the versioned fixtures. (3a8a692f)
- [x] Type planning and run commands, deferred arguments, and scheduler result statuses. Expand the mypy check to 22 modules. (159cd8ce)
- [ ] Distinguish confirmed artifact stamps from possibly missing stamps and check more modules with mypy.
- [ ] Verify PR CI and run the Slurm scale and cluster checks before merging.
