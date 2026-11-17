import sys

if sys.argv[1:2] == ["job"]:
    # Before the imports below, which can take seconds on a shared filesystem: see `slurm.early`.
    from .slurm import early

    early.install()

from .cli import main  # noqa: E402

raise SystemExit(main())
