"""The controller's side of a run that uses Slurm: its profile and ledger, what it does before planning, and the
executor it gives the scheduler."""

from __future__ import annotations

import asyncio
import json
import os
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from ..execute.executor import Executor, Local, Mixed
from ..execute.options import Options
from ..execute.scheduler import MakeExecutor, Result
from .. import runs
from ..model import Dag, DagError, Layout
from ..plan import Decision, Status
from ..schedule import ReadyQueue
from ..state.record import Record
from ..state.store import Store
from . import commands
from .executor import Slurm, job_name
from .harvest import Harvest, harvest
from .ledger import End, Entry, Ledger
from .profile import Profile


# How many short jobs of one operation a run notes.
SHORT_COUNT = 5


# How often a controller says it is alive, and how long after it last did another controller of the folder refuses to
# start.
BEAT_SECONDS = 60.0
ALIVE_SECONDS = 300.0


def _alive(other: dict, host: str) -> bool:
    """Whether the controller `other` recorded may be running: on this host, whether its process is; on another, yes."""
    if other.get("host") != host:
        return True
    try:
        os.kill(int(other.get("pid", 0)), 0)
    except (ProcessLookupError, ValueError):
        return False
    except PermissionError:
        return True
    return True


def ledger_path(layout: Layout) -> Path:
    return layout.folder / "slurm" / "ledger.jsonl"


@dataclass
class Controller:
    """A run's Slurm controller for the runner folder `layout`."""

    profile: Profile
    layout: Layout
    ledger: Ledger
    # This process as `claim` recorded it, and when it last said it was alive.
    _me: dict | None = None
    _beaten: float = 0.0

    @property
    def folder(self) -> Path:
        return self.layout.folder / "slurm"

    @property
    def name(self) -> str:
        return job_name(self.layout.folder)

    def claim(self) -> None:
        """Record this process as the folder's controller, or refuse if another one, on any host, said it was alive
        within `ALIVE_SECONDS`.

        The state's lock cannot be relied on to keep two hosts out: on some shared filesystems a lock holds only on
        the host that took it. Two controllers would each cancel the other's tasks as strays, and write the same
        outputs. Raises `DagError`.
        """
        path = self.folder / "controller.json"
        try:
            other = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            other = None
        me = {"host": socket.gethostname(), "pid": os.getpid()}
        if (isinstance(other, dict) and {"host": other.get("host"), "pid": other.get("pid")} != me
                and time.time() - float(other.get("at", 0)) < ALIVE_SECONDS and _alive(other, me["host"])):
            raise DagError(f"another spit-bash run, process {other.get('pid')} on {other.get('host')}, controls this "
                           f"folder's Slurm tasks, and said so {time.time() - float(other['at']):.0f}s ago. Wait for "
                           f"it to end; if it was killed, run again once {ALIVE_SECONDS / 60:.0f} minutes have passed "
                           "since then.")
        self._me = me
        self.beat(force=True)

    def beat(self, force: bool = False) -> None:
        """Say this controller is alive, at most once every `BEAT_SECONDS` unless `force`."""
        if self._me is None or (not force and time.time() - self._beaten < BEAT_SECONDS):
            return
        self._beaten = time.time()
        path = self.folder / "controller.json"
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_name(f".{path.name}.{os.getpid()}")
            temporary.write_text(json.dumps({**self._me, "at": self._beaten}), encoding="utf-8")
            os.replace(temporary, path)
        except OSError:
            pass

    def release(self) -> None:
        """Say no controller runs, as this one ends."""
        if self._me is not None:
            (self.folder / "controller.json").unlink(missing_ok=True)
            self._me = None

    def harvest(self, dag: Dag, state: Store, write: bool) -> Harvest:
        return asyncio.run(harvest(dag, state, self.ledger, self.name, self.profile.result_wait, write, self.beat))

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
            found = await commands.queue(self.name)
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
                           adopted=adopted, records=state.jobs, beat=self.beat)
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
        self.release()


def settle_outputs(dag: Dag, state: Store, runs_folder: Path, window: float) -> int:
    """Wait until this machine sees the outputs the latest Slurm run recorded, when it ended within `window` seconds;
    how many were missing at first.

    The jobs wrote them on other nodes, and this one may have looked them up before they existed, as the run's own
    plan did, and cache that they were missing: a plan made then would rerun finished jobs. Each output a record of
    that run holds but this machine does not see is looked for, by listing its folder, every two seconds until it
    shows or the window closes. An output deleted since costs at most what is left of the window.
    """
    latest = next((run for run in runs.history(runs_folder) if (run.folder / "slurm").is_dir()), None)
    if latest is None or not latest.summary.exists():
        return 0
    closes = latest.summary.stat().st_mtime + window
    if time.time() >= closes:
        return 0
    missing = []
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is None or record.run != latest.name:
            continue
        recorded = set((record.output_paths or {}).values())
        missing += [path for path in job.output_paths if path in recorded and not dag.path(path).exists()]
    waiting = list(missing)
    while waiting and time.time() < closes:
        time.sleep(2.0)
        for folder in {dag.path(path).parent for path in waiting}:
            try:
                os.listdir(folder)
            except OSError:
                pass
        waiting = [path for path in waiting if not dag.path(path).exists()]
    return len(missing)


def open_controller(layout: Layout, profile: Profile, write: bool) -> Controller:
    """The controller of the runner folder `layout`; without `write`, as for `plan`, its ledger is only read."""
    return Controller(profile, layout, Ledger(ledger_path(layout), write))


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
