"""The controller's side of a run that uses Slurm: its lease on the folder, what it reads of the queue and the results
before planning, and the executor it gives the scheduler."""

from __future__ import annotations

import asyncio
import json
import os
import socket
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

from ..execute.executor import Executor, Local, Mixed
from ..execute.options import Options
from ..execute.scheduler import MakeExecutor, Result as Ran
from ..model import Dag, DagError, Layout
from ..plan import Decision, Status
from ..reuse import records
from ..schedule import ReadyQueue
from ..state.record import Record
from ..state.store import StateWrite, Store
from . import claim, commands
from . import result as results
from .executor import Slurm, identify, job_name
from .intent import Manifests
from .node import visible
from .profile import Profile
from .reconcile import Identity, Mode, Task, reconcile, releasable
from .result import Outcome, read

# How many short jobs of one operation a run notes.
SHORT_COUNT = 5
# How often a controller says it is alive, and how long after it last did another controller of the folder refuses to
# start.
BEAT_SECONDS = 60.0
ALIVE_SECONDS = 300.0


def folder(layout: Layout) -> Path:
    """The runner folder's Slurm folder, whose being there means a run has used Slurm here."""
    return layout.folder / "slurm"


def planned(decisions: tuple[Decision, ...]) -> dict[str, Identity]:
    """What the run does for each key it runs or checks: the identity the queue is kept to."""
    found = {}
    for decision in decisions:
        work = decision.job.work
        if decision.status in (Status.RUN, Status.MAYBE):
            found[work.key] = Identity(work.key, work.fingerprint, Mode.RUN)
        elif decision.needs_checks:
            found[work.key] = Identity(work.key, work.fingerprint, Mode.CHECK)
    return found


@dataclass
class Start:
    """The queue and the claims as the run found them, and what it made of the results waiting."""

    tasks: tuple[Task, ...]
    held: dict[str, str]
    lines: list[str]

    @property
    def live(self) -> int:
        """How many tasks of this folder's intents are queued or running."""
        return sum(1 for task in self.tasks if task.live and task.intent is not None)


@dataclass
class Controller:
    """A run's Slurm controller for the runner folder `layout`."""

    profile: Profile
    layout: Layout
    config: commands.Config = field(default_factory=commands.Config)
    # This process as `lease` recorded it, and when it last said it was alive.
    _me: dict | None = None
    _beaten: float = 0.0

    @property
    def folder(self) -> Path:
        return folder(self.layout)

    @property
    def name(self) -> str:
        return job_name(self.layout.folder)

    def lease(self) -> None:
        """Record this process as the folder's controller, or refuse if another one, on any host, said it was alive
        within `ALIVE_SECONDS`. Raises `DagError`.

        The state's lock cannot keep two hosts out on every shared filesystem: on some, a lock holds only on the host
        that took it. Two controllers would cancel each other's tasks as work their plans do not want.
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

    def start(self, dag: Dag, state: Store, write: bool) -> Start:
        """Read the queue and the results waiting, before planning. Without `write`, as for `plan`, change nothing.

        Each key's newest success whose task has ended, for the work the DAG now has, is recorded, as the run that
        submitted it would have: a job that finished while no controller watched is not run again. A result of a live
        task is left for the executor, which waits on that task. Every other result is deleted, and with it every
        intent no task in the queue belongs to, and every claim no live task holds. Raises `DagError` when a run
        cannot read the queue.
        """
        self.config = commands.config()
        held = claim.holders(self.folder)
        listed = asyncio.run(commands.snapshot(self.name))
        if listed is None:
            if write:
                raise DagError("cannot read the Slurm queue with squeue, which a run of this folder needs to know "
                               "what its earlier runs left; run again once squeue answers")
            return Start((), {}, ["cannot read the Slurm queue: the plan does not count tasks still in it"])
        manifests = Manifests(self.folder)
        tasks = identify(listed, manifests)
        live = {task.id for task in tasks if task.live}
        jobs = {job.work.key: job for job in dag.jobs}
        found: dict[str, list[tuple[Task, results.Result, Path]]] = {}
        try:
            names = sorted(os.listdir(results.folder(self.folder)))
        except OSError:
            names = []
        for name in names:
            path = results.folder(self.folder) / name
            if name.removesuffix(".json") in live:
                continue
            try:
                result = read(path)
            except (OSError, ValueError):
                result = None
            if result is None or result.outcome not in (Outcome.MADE, Outcome.CHECKED):
                if write:
                    path.unlink(missing_ok=True)
                continue
            found.setdefault(result.key, []).append((Task(result.task, result.intent, result.index, ""), result, path))
        recorded = 0
        for key, each in found.items():
            each.sort(key=lambda item: item[0].order)
            _, newest, _ = each[-1]
            job = jobs.get(key)
            if (newest.outcome is Outcome.MADE and newest.finished is not None and job is not None
                    and job.work.fingerprint == newest.fingerprint):
                made = newest.finished.made
                record = records.after_run(job, made.inputs, made.outputs, newest.finished.input_hashes,
                                           newest.finished.output_hashes, made.seconds, newest.run)
                if write:
                    state.record(key, record)
                else:
                    state.assume((StateWrite(key, record),))
                recorded += 1
            if write:
                for _, _, path in each:
                    path.unlink(missing_ok=True)
        if write:
            manifests.remove_unused({task.intent for task in tasks if task.intent is not None})
            for each in releasable(held, tasks):
                claim.release(self.folder, each)
        start = Start(tasks, held, [])
        if recorded:
            start.lines.append(f"recorded {recorded} job{'' if recorded == 1 else 's'} that finished in Slurm while no "
                               "run watched")
        if start.live:
            start.lines.append(f"{start.live} Slurm task{'' if start.live == 1 else 's'} of this folder "
                               f"{'is' if start.live == 1 else 'are'} queued or running")
        return start

    def act(self, wanted: dict[str, Identity], start: Start) -> int:
        """Cancel every task in the queue that does no work the run wants; how many. Done once the plan is made, so
        that a run with nothing to submit still leaves no task of older work running."""
        claims = {key: start.held[claim.name(key)] for key in wanted if claim.name(key) in start.held}
        cancel = reconcile(wanted, start.tasks, Manifests(self.folder).identify, claims).cancel
        if cancel:
            commands.cancel([task.id for task in cancel])
        return len(cancel)

    def executor(self, dag: Dag, options: Options, state: Store, wanted: dict[str, Identity],
                 start: Start) -> MakeExecutor:
        recorded = {key: record.seconds for key, record in state.jobs.items() if record.seconds is not None}

        def make(ranks: dict[int, float]) -> Executor:
            remote = Slurm(self.profile, dag, options, self.folder, ranks, recorded, state.jobs, wanted, start.tasks,
                           start.held, self.config, self.beat)
            if not self.profile.local:
                return remote
            return Mixed(Local(ReadyQueue(self.profile.local_cores, None, ranks)), remote, self.profile.local)
        return make

    def short_lines(self, ran: tuple[Ran, ...], records_now: dict[str, Record], run: str) -> list[str]:
        """A note for each operation whose jobs ran as Slurm jobs this run and took under `short_job` on average,
        when there are at least `SHORT_COUNT` of them: a scheduler spends more on starting each than they take."""
        seconds: dict[str, list[float]] = {}
        for each in ran:
            job = each.job
            record = records_now.get(job.work.key)
            if (each.failure is None and job.operation not in self.profile.local and record is not None
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
        """Say no controller runs, as this one ends."""
        if self._me is not None:
            (self.folder / "controller.json").unlink(missing_ok=True)
            self._me = None


def settle(dag: Dag, state: Store, bound: float) -> None:
    """Wait until this machine sees each output a record holds that was written within `bound` seconds and looks
    missing, or until it is `bound` old: so a plan made just after a Slurm run judges what the jobs wrote, not what
    this machine's cache shows. An output deleted since costs at most what is left of its bound."""
    now = time.time()
    expected = []
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is None:
            continue
        for slot, path in (record.output_paths or {}).items():
            stamp = record.outputs.get(slot)
            if (stamp is not None and path in job.output_paths and stamp[1] / 1e9 > now - bound
                    and not dag.path(path).exists()):
                expected.append((path, tuple(stamp)))
    if expected:
        asyncio.run(visible(dag, tuple(expected), bound))


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
