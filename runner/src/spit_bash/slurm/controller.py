"""The controller's side of a run that uses Slurm: its profile and ledger, what it does before planning, and the
executor it gives the scheduler."""

from __future__ import annotations

import asyncio
import os
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from ..execute.executor import Executor, Local, Mixed
from ..execute.options import Options
from ..execute.scheduler import MakeExecutor, Result
from ..model import Dag, DagError, Layout
from ..plan import Decision, Status
from ..schedule import ReadyQueue
from ..state.record import Record
from ..state.store import Store
from . import commands
from .executor import Slurm, job_names, namespace
from .harvest import Harvest, harvest
from .ledger import End, Entry, Ledger
from .profile import Profile


# How many short jobs of one operation a run notes.
SHORT_COUNT = 5


def ledger_path(layout: Layout) -> Path:
    return layout.folder / "slurm" / "ledger.jsonl"


@dataclass
class Controller:
    """A run's Slurm controller for the runner folder `layout`."""

    profile: Profile
    layout: Layout
    ledger: Ledger
    # The DAG's job names, for squeue to filter by, once `harvest` has seen the DAG.
    names: tuple[str, ...] = ()

    @property
    def folder(self) -> Path:
        return self.layout.folder / "slurm"

    @property
    def prefix(self) -> str:
        return f"spit-{namespace(self.layout.folder)}-"

    def harvest(self, dag: Dag, state: Store, write: bool) -> Harvest:
        self.names = job_names(self.prefix, dag)
        return asyncio.run(harvest(dag, state, self.ledger, self.prefix, self.profile.result_wait, write, self.names))

    def cancel_unneeded(self, live: dict[str, Entry], decisions: tuple[Decision, ...]) -> int:
        """Cancel the live tasks of jobs this run does not run, which would otherwise write their outputs under it,
        and wait until they have left the queue; how many. Those left are taken over by the run.

        A job planned only for its checks runs no commands, so a task running its commands is cancelled too.
        Raises `DagError` when a cancelled task is still queued after `result_wait`.
        """
        running = {decision.job.work.key for decision in decisions
                   if decision.status in (Status.RUN, Status.MAYBE)}
        unneeded = [entry for key, entry in live.items() if key not in running]
        if not unneeded:
            return 0
        tasks = [entry.task for entry in unneeded]
        commands.cancel(tasks)
        asyncio.run(self._gone(tasks))
        for entry in unneeded:
            self.ledger.ended(entry.token, End.CANCELLED)
            del live[entry.key]
        return len(unneeded)

    async def _gone(self, tasks: list[str]) -> None:
        """Wait, with one squeue call every ten seconds, until none of `tasks` is in the queue: a cancelled task
        puts back its job's earlier outputs as it stops, which a job of this run may read."""
        deadline = time.monotonic() + self.profile.result_wait
        while True:
            found = await commands.queue(self.prefix, self.names)
            if found is not None and not any(task in found for task in tasks):
                return
            if time.monotonic() >= deadline:
                raise DagError(f"{len(tasks)} cancelled Slurm task(s) an earlier run submitted are still in the queue "
                               f"after {self.profile.result_wait:.0f}s; run again once they have left")
            await asyncio.sleep(10.0)

    def executor(self, dag: Dag, options: Options, state: Store, adopted: dict[str, Entry]) -> MakeExecutor:
        recorded = {key: record.seconds for key, record in state.jobs.items() if record.seconds is not None}

        def make(ranks: dict[int, float]) -> Executor:
            remote = Slurm(self.profile, dag, options, self.folder, self.ledger, ranks, recorded,
                           adopted=adopted, records=state.jobs)
            if not self.profile.local:
                return remote
            return Mixed(Local(ReadyQueue(self.profile.local_cores, None, ranks)), remote, self.profile.local)
        return make

    def short_lines(self, results: tuple[Result, ...], records: dict[str, Record], run: str) -> list[str]:
        """A note for each operation whose jobs ran as Slurm jobs this run and took under `short_job` on average,
        when there are at least `SHORT_COUNT` of them: a scheduler spends more on starting each than they take."""
        seconds: dict[str, list[float]] = {}
        for result in results:
            job = result.job
            record = records.get(job.work.key)
            if (result.failure is None and job.operation not in self.profile.local and record is not None
                    and record.run == run and record.seconds is not None):
                seconds.setdefault(job.operation, []).append(record.seconds)
        lines = []
        for operation, times in sorted(seconds.items()):
            mean = sum(times) / len(times)
            if len(times) >= SHORT_COUNT and mean < self.profile.short_job:
                lines.append(f"note: {len(times)} jobs of {operation} took {mean:.1f}s each on average as Slurm jobs; "
                             f"a scheduler spends longer starting a job than that. Name {operation} under `local` "
                             "in the profile to run its jobs here instead.")
        return lines

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
