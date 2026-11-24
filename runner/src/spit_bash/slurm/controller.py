"""The controller's side of a run that uses Slurm: its lease on the folder, what it reads of the queue and the results
before planning, and the executor it gives the scheduler."""

from __future__ import annotations

import asyncio
import json
import os
import socket
import subprocess
import time
import uuid
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
from .dispatch import Dispatches, terminal_accounting
from .executor import Slurm, holders, identify, job_name
from .intent import Manifests
from .node import visible
from .profile import Profile
from .reconcile import Identity, Mode, Task, reconcile, releasable
from .result import Outcome, read

# How many short jobs of one operation a run notes.
SHORT_COUNT = 5


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
    # This process and lease nonce, as recorded on the shared filesystem.
    _me: dict | None = None

    @property
    def folder(self) -> Path:
        return folder(self.layout)

    @property
    def name(self) -> str:
        return job_name(self.layout.folder)

    def lease(self) -> None:
        """Acquire an exclusive shared-filesystem lease before changing Slurm state."""
        path = self.folder / "controller.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        me = {"host": socket.gethostname(), "pid": os.getpid(), "nonce": uuid.uuid4().hex}
        try:
            other = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            pass
        except (OSError, ValueError) as exc:
            raise DagError(f"cannot verify Slurm controller lease {path}: {exc}") from exc
        else:
            if not isinstance(other, dict) or _alive(other, me["host"]):
                who = f"process {other.get('pid')} on {other.get('host')}" if isinstance(other, dict) else "unknown"
                raise DagError(f"another Slurm controller ({who}) holds {path}; verify it has stopped before "
                               "removing the lease on another host")
            # The caller holds the local state lock. Only its own host can verify that a PID has gone.
            path.unlink()
        try:
            with path.open("x", encoding="utf-8") as stream:
                json.dump(me, stream)
                stream.flush()
                os.fsync(stream.fileno())
        except FileExistsError as exc:
            raise DagError(f"another Slurm controller acquired {path}") from exc
        self._me = me

    def check_lease(self) -> None:
        """Verify this controller still holds its lease before contacting Slurm to change jobs."""
        path = self.folder / "controller.json"
        try:
            if self._me is not None and json.loads(path.read_text(encoding="utf-8")) == self._me:
                return
        except (OSError, ValueError) as exc:
            raise DagError(f"cannot verify Slurm controller lease {path}: {exc}") from exc
        raise DagError(f"controller no longer holds Slurm lease {path}")

    def start(self, dag: Dag, state: Store, write: bool) -> Start:
        """Read the queue and the results waiting, before planning. Without `write`, as for `plan`, change nothing.

        Each key's newest success whose task has ended, for the work the DAG now has, is recorded, as the run that
        submitted it would have: a job that finished while no controller watched is not run again. A result of a live
        task is left for the executor, which waits on that task. Every other result is deleted, and with it every
        intent no task in the queue belongs to, and every claim no live task holds. Raises `DagError` when a run
        cannot read the queue.
        """
        commands.require_local_cluster()
        self.config = commands.config()
        held = claim.holders(self.folder)
        listed = asyncio.run(commands.snapshot(self.name, (), "start"))
        if listed is None:
            if write:
                raise DagError("cannot read the Slurm queue with squeue, which a run of this folder needs to know "
                               "what its earlier runs left; run again once squeue answers")
            return Start((), {}, ["cannot read the Slurm queue: the plan does not count tasks still in it"])
        manifests = Manifests(self.folder)
        tasks = identify(listed, manifests)
        dispatches = Dispatches(self.folder)
        dispatches.reconcile(tasks, write=write)
        ended = asyncio.run(terminal_accounting(dispatches.missing(tasks)))
        tasks += ended
        dispatches.reconcile(ended, write=write)
        present = {(task.id, task.intent) for task in tasks}
        hidden = tuple(task for task in dispatches.tasks().values() if (task.id, task.intent) not in present)
        warnings = []
        if dispatches.unknown:
            warnings.append(f"Slurm acceptance unresolved for {', '.join(dispatches.unknown)} in {self.folder}")
        if hidden:
            warnings.append("cannot verify Slurm state for " + ", ".join(task.id for task in hidden))
        if warnings and write:
            raise DagError(". ".join(warnings) + "; reservations kept and no new work submitted")
        tasks += hidden
        live = {task.id for task in tasks if task.live}
        latest = {task.id: task.intent for task in tasks if task.intent is not None}
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
            if result.task in latest and result.intent != latest[result.task]:
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
                # The same record a controller that watched the task would make, from what the task carried.
                record = records.after_run(job, made.inputs, made.outputs, newest.finished.input_hashes,
                                           newest.finished.output_hashes, made.seconds, newest.run,
                                           newest.attempt, newest.memory, newest.time_limit)
                if write:
                    state.record(key, record)
                else:
                    state.assume((StateWrite(key, record),))
                recorded += 1
            if write:
                for _, _, path in each:
                    path.unlink(missing_ok=True)
        if write:
            manifests.remove_unused({task.intent for task in tasks if task.intent is not None and task.live}
                                    | dispatches.records.keys())
            for each in releasable(held, tasks):
                claim.release(self.folder, each)
        start = Start(tasks, held, warnings)
        if recorded:
            start.lines.append(f"recorded {recorded} job{'' if recorded == 1 else 's'} that finished in Slurm while no "
                               "run watched")
        if start.live:
            start.lines.append(f"{start.live} Slurm task{'' if start.live == 1 else 's'} of this folder "
                               f"{'is' if start.live == 1 else 'are'} queued or running")
        return start

    def act(self, wanted: dict[str, Identity], start: Start) -> int:
        """Request cancellation of obsolete work, and confirm it before admitting replacement work."""
        manifests = Manifests(self.folder)
        identify_task = manifests.identify
        cancel = reconcile(wanted, start.tasks, identify_task, holders(start.tasks, identify_task, start.held)).cancel
        if not cancel:
            return 0
        self.check_lease()
        pending = {task.id: task for task in cancel}
        latest = commands.snapshot_now(self.name, tuple(pending), "cancel")
        if latest is None:
            raise DagError("cannot verify Slurm cancellation targets; no new work submitted")
        verified = identify(latest, manifests)
        targets = [task.id for task in verified if task.id in pending and task.live
                   and task.intent == pending[task.id].intent and task.intent is not None]
        self.check_lease()
        failed = commands.cancel(targets)
        if failed:
            raise DagError(f"could not request Slurm cancellation for {', '.join(failed)}; no new work submitted")
        try:
            confirmed = asyncio.run(asyncio.wait_for(self._confirm_cancel(pending, start, manifests),
                                                     timeout=self.profile.visibility))
        except asyncio.TimeoutError:
            confirmed = False
        if not confirmed:
            raise DagError(f"Slurm cancellation unconfirmed for {', '.join(pending)}; reservations kept")
        return len(cancel)

    async def _confirm_cancel(self, pending: dict[str, Task], start: Start, manifests: Manifests) -> bool:
        dispatches = Dispatches(self.folder)
        pause = min(max(1.0, min(self.profile.status_interval, 5.0)), self.profile.visibility / 2)
        while pending:
            listed = await commands.snapshot(self.name, tuple(pending), "cancel")
            if listed is None:
                return False
            visible = identify(listed, manifests)
            present = {task.id for task in visible}
            ended = await terminal_accounting({id: task for id, task in pending.items() if id not in present})
            confirmed = visible + ended
            unchanged = tuple(task for task in start.tasks if task.id not in pending)
            start.tasks = unchanged + tuple(task for task in confirmed if task.id in pending)
            dispatches.reconcile(confirmed)
            for key in releasable(start.held, confirmed):
                claim.release(self.folder, key)
            for task in confirmed:
                previous = pending.get(task.id)
                if previous is not None and task.intent == previous.intent and not task.live:
                    del pending[task.id]
            if pending:
                await asyncio.sleep(pause)
                pause *= 2
        return True

    def executor(self, dag: Dag, options: Options, state: Store, wanted: dict[str, Identity],
                 start: Start) -> MakeExecutor:
        recorded = {key: record.seconds for key, record in state.jobs.items() if record.seconds is not None}

        def make(ranks: dict[int, float]) -> Executor:
            remote = Slurm(self.profile, dag, options, self.folder, ranks, recorded, state.jobs, wanted, start.tasks,
                           start.held, self.config, self.check_lease)
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
        """Release the lease if still owned by this controller."""
        if self._me is not None:
            try:
                self.check_lease()
            except DagError:
                pass
            else:
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
    pid = other.get("pid")
    if other.get("host") != host or type(pid) is not int or pid < 1:
        return True
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except (PermissionError, OverflowError):
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
