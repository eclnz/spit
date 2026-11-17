"""The controller's side of a run that uses Slurm: its profile and ledger, what it does before planning, and the
executor it gives the scheduler."""

from __future__ import annotations

import asyncio
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

from ..execute.executor import Executor, Local, Mixed
from ..execute.options import Options
from ..execute.scheduler import MakeExecutor
from ..model import Dag, DagError, Layout
from ..plan import Decision, Status
from ..schedule import ReadyQueue
from ..state.store import Store
from . import commands
from .executor import Slurm, namespace
from .harvest import Harvest, harvest
from .ledger import End, Entry, Ledger
from .profile import Profile


def ledger_path(layout: Layout) -> Path:
    return layout.folder / "slurm" / "ledger.jsonl"


@dataclass
class Controller:
    """A run's Slurm controller for the runner folder `layout`."""

    profile: Profile
    layout: Layout
    ledger: Ledger

    @property
    def folder(self) -> Path:
        return self.layout.folder / "slurm"

    @property
    def prefix(self) -> str:
        return f"spit-{namespace(self.layout.folder)}-"

    def harvest(self, dag: Dag, state: Store, write: bool) -> Harvest:
        return asyncio.run(harvest(dag, state, self.ledger, self.prefix, self.profile.result_wait, write))

    def cancel_unneeded(self, live: dict[str, Entry], decisions: tuple[Decision, ...]) -> int:
        """Cancel the live tasks of jobs this run does not run, which would otherwise write their outputs under it;
        how many. Those left are taken over by the run."""
        running = {decision.job.work.key for decision in decisions
                   if decision.status in (Status.RUN, Status.MAYBE) or decision.needs_checks}
        unneeded = [entry for key, entry in live.items() if key not in running]
        if unneeded:
            commands.cancel([entry.task for entry in unneeded])
            for entry in unneeded:
                self.ledger.ended(entry.token, End.CANCELLED)
                del live[entry.key]
        return len(unneeded)

    def executor(self, dag: Dag, options: Options, state: Store, adopted: dict[str, Entry]) -> MakeExecutor:
        recorded = {key: record.seconds for key, record in state.jobs.items() if record.seconds is not None}

        def make(ranks: dict[int, float]) -> Executor:
            remote = Slurm(self.profile, dag, options, self.folder, self.ledger, ranks, recorded, adopted)
            if not self.profile.local:
                return remote
            return Mixed(Local(ReadyQueue(self.profile.local_cores, None, ranks)), remote, self.profile.local)
        return make

    def close(self) -> None:
        self.ledger.close()


def open_controller(layout: Layout, profile: Profile) -> Controller:
    return Controller(profile, layout, Ledger(ledger_path(layout)))


def set_up_path(profile: Profile) -> None:
    """Put on this process's PATH what the profile's setup lines add to it, so the plan finds programs that appear
    only after a `module load`, as the jobs will.

    Every operation's setup lines are run, each set once, in a login shell, and the PATH each leaves is joined, so
    a program found here may still be missing for an operation whose own lines do not provide it: that job then
    fails when it runs.
    """
    sets = {profile.resources.setup, *(profile.resources_for_operation(name).setup for name in profile.operations)}
    sets.discard(())
    found = [os.environ.get("PATH", "")]
    for lines in sorted(sets):
        script = "\n".join((*lines, 'printf "%s" "$PATH"'))
        try:
            done = subprocess.run(["bash", "-lc", script], capture_output=True, text=True, timeout=120)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise DagError(f"cannot run the profile's setup lines: {exc}") from exc
        if done.returncode != 0:
            raise DagError(f"the profile's setup lines exited with status {done.returncode}: "
                           f"{done.stderr.strip().splitlines()[-1] if done.stderr.strip() else 'no message'}")
        found.append(done.stdout.strip())
    joined: list[str] = []
    for path in found:
        for part in path.split(os.pathsep):
            if part and part not in joined:
                joined.append(part)
    os.environ["PATH"] = os.pathsep.join(joined)
