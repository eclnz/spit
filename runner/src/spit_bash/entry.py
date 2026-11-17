"""The `spit-bash` program, for the console script and `python -m spit_bash` alike."""

import sys


def main() -> int:
    if sys.argv[1:2] == ["job"]:
        # Before the imports below, which can take seconds on a shared filesystem: see `slurm.early`.
        from .slurm import early

        early.install()
    from .cli import main as run

    return run()
