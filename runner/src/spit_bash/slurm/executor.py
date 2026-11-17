"""The Slurm executor: submits ready jobs as Slurm arrays, and learns how each ended from its result file.

A job's task runs `spit-bash job` on a compute node, which writes the result; the controller reads the results
folder often and the queue rarely, in one `squeue` call for the whole run, and asks `sacct` only why a task that
left the queue without a result ended. Every write to the state stays with the scheduler, on the controller.
"""

from __future__ import annotations

import asyncio
import collections
import hashlib
import os
import secrets
import shlex
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Union

from ..execute.executor import Hooks, Ran, Task
from ..execute.failure import CheckFailed, CommandFailed, Failure, SlurmEnded
from ..execute.job import Failed
from ..execute.options import Options
from ..execute.process import KilledSignal
from ..model import Dag, Job
from ..schedule import ReadyQueue, size
from ..state.record import Record
from . import batch as batches
from . import commands
from .batch import Batch, Mode, Spec
from .ledger import End, Entry, Ledger
from .profile import Profile, Resources, megabytes, minutes
from .result import Outcome, Result, read

# How many times an sbatch call that may or may not have been accepted is tried again before the jobs fail.
UNSURE_TRIES = 4


def namespace(folder: Path) -> str:
    """Eight hex digits naming the runner folder `folder`, which the run's Slurm jobs carry in their names.

    The same folder always gives the same namespace, so a controller finds the tasks an earlier one submitted, and two
    folders' runs never take each other's tasks.
    """
    return hashlib.sha1(str(folder).encode()).hexdigest()[:8]


class CountQueue(ReadyQueue):
    """A ready queue that counts tasks, not cores: a job takes one place of `places`, whatever it declares."""

    def __init__(self, places: int, ranks: dict[int, float]) -> None:
        super().__init__(places, None, ranks)

    def needs(self, job: Job) -> tuple[int, int]:
        return 1, 0


@dataclass(frozen=True)
class Lost:
    """A task left the queue without a result, or its result could not be read; `state` is sacct's, when it said."""

    state: str | None
    words: str


Ended = Union[Result, Lost]


@dataclass
class Waiter:
    """A submitted task the controller waits on."""

    entry: Entry
    future: asyncio.Future[Ended]
    hooks: Hooks
    # When the queue was first seen without it, as `time.monotonic`; None while it is listed.
    gone: float | None = None
    started: bool = False


@dataclass
class Pending:
    """A task gathering with others of its operation and resources, to be submitted as one array."""

    spec: Spec
    job: Job
    time: float
    future: asyncio.Future[Ended]
    hooks: Hooks


@dataclass(frozen=True)
class Group:
    """What the tasks of one array share: Slurm takes one set of options for all of an array's tasks."""

    operation: str
    cores: int
    memory: int
    resources: Resources


@dataclass
class Slurm:
    """Runs jobs as tasks of Slurm arrays; `adopted` are tasks an earlier controller left queued, by job key."""

    profile: Profile
    dag: Dag
    options: Options
    folder: Path
    ledger: Ledger
    ranks: dict[int, float]
    # Each job's time from its last run, by state key, for its `--time`.
    recorded: dict[str, float]
    # The state's records, which the scheduler updates as jobs succeed, for the stamps a job's inputs should have.
    records: dict[str, Record] = field(default_factory=dict)
    adopted: dict[str, Entry] = field(default_factory=dict)
    queue: CountQueue = field(init=False)
    prefix: str = field(init=False)
    waiting: dict[str, Waiter] = field(default_factory=dict, init=False)
    gathering: dict[Group, list[Pending]] = field(default_factory=dict, init=False)
    sent: collections.deque[float] = field(default_factory=collections.deque, init=False)
    _monitor: asyncio.Task[None] | None = field(default=None, init=False)
    _neighbours: dict[Path, list[str]] | None = field(default=None, init=False)
    _max_array: int | None = field(default=None, init=False)
    _last_status: float = field(default=0.0, init=False)
    # The job that makes each output path.
    _made_by: dict[str, Job] = field(default_factory=dict, init=False)
    # Each task whose end was dealt with, by token, for asking sacct why it ended.
    done: dict[str, Entry] = field(default_factory=dict, init=False)

    def __post_init__(self) -> None:
        self.queue = CountQueue(self.profile.jobs, self.ranks)
        self.prefix = f"spit-{namespace(self.folder.parent)}-"
        self._made_by = {path: job for job in self.dag.jobs for path in job.output_paths}

    @property
    def results(self) -> Path:
        return self.folder / "results"

    def over(self, job: Job) -> str | None:
        return None

    @property
    def width(self) -> int:
        return self.profile.jobs

    async def run(self, dag: Dag, task: Task, log: Path, options: Options, hooks: Hooks) -> Ran:
        job = task.job
        async with self.queue.slot(job):
            hooks.admitted()
            resources = self.profile.resources_for(job)
            memory = job.memory if job.memory is not None else resources.memory
            limit = self.profile.time_for(job, self.recorded.get(job.work.key), resources)
            adopted = await self._adopt(job, hooks)
            attempt, ended = adopted if adopted is not None else (1, None)
            while True:
                attempt_log = _attempt_log(log, attempt)
                if ended is None:
                    spec = self._spec(job, Mode.RUN, attempt, attempt_log, task)
                    ended = await self._submit(job, spec, limit, memory, resources, hooks)
                if isinstance(ended, Result) and ended.outcome is Outcome.MADE:
                    assert ended.finished is not None, "a result of a job that made its outputs holds them"
                    return ended.finished
                state = await self._state(ended)
                retry = self._retry(state, ended, attempt)
                if retry is None:
                    return Failed(self._failure(ended, state), attempt_log)
                why, more_memory, more_time = retry
                memory = int(memory * self.profile.memory_factor) if more_memory else memory
                limit = limit * self.profile.retry_time_factor if more_time else limit
                attempt += 1
                hooks.retrying(attempt, why, f"{size(memory)} of memory and {minutes(limit)} minutes")
                with open(_attempt_log(log, attempt), "w", encoding="utf-8") as note:
                    note.write(f"spit-bash: attempt {attempt}: Slurm ended attempt {attempt - 1} {why}; "
                               f"asking for {size(memory)} of memory and {minutes(limit)} minutes\n")
                ended = None

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        async with self.queue.slot(job):
            hooks.admitted()
            resources = self.profile.resources_for(job)
            memory = job.memory if job.memory is not None else resources.memory
            limit = self.profile.time_for(job, None, resources)
            ended = await self._submit(job, self._spec(job, Mode.CHECK, 1, log, Task(job)), limit, memory, resources,
                                       hooks)
            if isinstance(ended, Result) and ended.outcome is Outcome.CHECKED:
                return None
            return self._failure(ended, await self._state(ended))

    def stop(self) -> None:
        """Cancel every task still queued or running, as a run that is stopped does; the next run starts them again."""
        if self._monitor is not None:
            self._monitor.cancel()
        tasks = [waiter.entry.task for waiter in self.waiting.values()]
        if tasks:
            commands.cancel(tasks)
        for token in list(self.waiting):
            self.ledger.ended(token, End.CANCELLED)
        self.waiting.clear()

    # Submitting.

    def _spec(self, job: Job, mode: Mode, attempt: int, log: Path, task: Task) -> Spec:
        token = f"{job.id}-{attempt}-{secrets.token_hex(4)}"
        return Spec(job.id, token, attempt, mode, self.results / f"{token}.json", log, task.reason, task.keep_edited,
                    self._expected(job))

    def _expected(self, job: Job) -> tuple[tuple[str, tuple[int, ...]], ...]:
        """Each of the job's inputs that another job makes, with the stamp that job's record gives it.

        The node that runs the job waits until it sees each so: the job that made it may have run on another
        node moments ago, whose writes this one's cache has not shown yet. A source has no record and no wait.
        """
        expected = []
        for path in dict.fromkeys(job.input_paths):
            maker = self._made_by.get(path)
            record = None if maker is None else self.records.get(maker.work.key)
            if maker is None or record is None:
                continue
            found = record.outputs.get(maker.identities[path])
            if found is not None:
                expected.append((path, tuple(found)))
        return tuple(expected)

    async def _submit(self, job: Job, spec: Spec, limit: float, memory: int, resources: Resources,
                      hooks: Hooks) -> Ended:
        """Gather the task with others like it, submit them as one array, and wait for its end."""
        future: asyncio.Future[Ended] = asyncio.get_running_loop().create_future()
        group = Group(job.operation, job.cores, memory, resources)
        gathering = self.gathering.setdefault(group, [])
        gathering.append(Pending(spec, job, limit, future, hooks))
        if len(gathering) == 1:
            asyncio.get_running_loop().call_later(self.profile.gather, lambda: asyncio.ensure_future(self._flush(group)))
        self._watch()
        return await future

    async def _flush(self, group: Group) -> None:
        pending = [each for each in self.gathering.pop(group, []) if not each.future.done()]
        if self._max_array is None:
            self._max_array = await asyncio.to_thread(commands.max_array_size) or 1001
        for start in range(0, len(pending), self._max_array):
            await self._send(group, pending[start:start + self._max_array])

    async def _rate(self) -> None:
        """Wait until another sbatch call keeps within the profile's submission rate."""
        while True:
            now = time.monotonic()
            while self.sent and now - self.sent[0] >= self.profile.per:
                self.sent.popleft()
            if len(self.sent) < self.profile.submissions:
                self.sent.append(now)
                return
            await asyncio.sleep(self.profile.per - (now - self.sent[0]))

    async def _send(self, group: Group, pending: list[Pending]) -> None:
        """Submit `pending` as one array, record each task in the ledger, and wait on it."""
        run = self.options.run.name
        token = secrets.token_hex(6)
        place = self.folder / "batches" / run
        if self._neighbours is None:
            self._neighbours = batches.by_folder(self.dag)
        trimmed = batches.trim(self.dag, [each.job for each in pending], self._neighbours)
        await asyncio.to_thread(batches.write, Batch(trimmed, self.options, tuple(each.spec for each in pending),
                                                     self.profile.result_wait), place / f"{token}.pickle")
        script = place / f"{token}.sh"
        command = shlex.join([*self.profile.command, "job", str(place / f"{token}.pickle")])
        script.write_text("#!/bin/bash\n" + "".join(f"{line}\n" for line in group.resources.setup)
                          + f'exec {command} "$SLURM_ARRAY_TASK_ID"\n', encoding="utf-8")
        (self.folder / "out" / run).mkdir(parents=True, exist_ok=True)
        self.results.mkdir(parents=True, exist_ok=True)
        resources = group.resources
        limit = f"%{self.profile.array_limit}" if self.profile.array_limit else ""
        options = [
            f"--array=0-{len(pending) - 1}{limit}", f"--job-name={self.prefix}{group.operation}",
            f"--comment=spit:{token}", f"--cpus-per-task={group.cores}", f"--mem={megabytes(group.memory)}M",
            f"--time={minutes(max(each.time for each in pending))}",
            f"--output={self.folder / 'out' / run / '%A_%a.out'}", "--no-requeue",
            f"--signal=B:TERM@{int(self.profile.grace)}",
            *([f"--account={resources.account}"] if resources.account else []),
            *([f"--partition={resources.partition}"] if resources.partition else []),
            *resources.options,
        ]
        array = await self._sbatch(script, options, token)
        if not isinstance(array, str):
            for each in pending:
                if not each.future.done():
                    each.future.set_result(Lost(None, f"could not submit it: {array}"))
            return
        for index, each in enumerate(pending):
            spec = each.spec
            entry = Entry(spec.token, f"{array}_{index}", each.job.id, each.job.work.key, each.job.work.fingerprint,
                          spec.attempt, str(spec.result), run, time.time())
            self.ledger.submitted(entry)
            self.waiting[spec.token] = Waiter(entry, each.future, each.hooks)
            each.hooks.queued(entry.task)
        # The monitor stops when nothing waits, which may have been while this array was being submitted.
        self._watch()

    async def _sbatch(self, script: Path, options: list[str], token: str) -> str | SlurmFailure:
        """Submit, waiting while the queue is full, and finding a submission sbatch may have made before failing."""
        tries = 0
        while True:
            await self._rate()
            try:
                return await commands.submit(script, options)
            except commands.Busy:
                await asyncio.sleep(self.profile.status_interval)
            except commands.Unsure as exc:
                found = await commands.queue(self.prefix)
                for queued in (found or {}).values():
                    if queued.comment == f"spit:{token}":
                        return queued.task.split("_")[0]
                tries += 1
                if tries >= UNSURE_TRIES:
                    return SlurmFailure(str(exc))
                await asyncio.sleep(min(self.profile.status_interval, 2.0 ** tries))
            except commands.SlurmError as exc:
                return SlurmFailure(str(exc))

    # Taking over a task an earlier controller submitted.

    async def _adopt(self, job: Job, hooks: Hooks) -> tuple[int, Ended] | None:
        """Wait on the task an earlier controller left queued for this job, if it runs this job's work: its attempt
        and how it ended. None to submit the job afresh, once a task that runs other work is cancelled and gone."""
        entry = self.adopted.pop(job.work.key, None)
        if entry is None:
            return None
        if entry.fingerprint != job.work.fingerprint:
            commands.cancel([entry.task])
            self.ledger.ended(entry.token, End.CANCELLED)
            await self._gone(entry.task)
            return None
        future: asyncio.Future[Ended] = asyncio.get_running_loop().create_future()
        self.waiting[entry.token] = Waiter(entry, future, hooks)
        hooks.queued(entry.task)
        self._watch()
        return entry.attempt, await future

    async def _gone(self, task: str) -> None:
        """Wait until `task` has left the queue, so that it no longer writes the job's outputs."""
        while True:
            found = await commands.queue(self.prefix)
            if found is not None and task not in found:
                return
            await asyncio.sleep(min(self.profile.status_interval, 5.0))

    # Watching.

    def _watch(self) -> None:
        if self._monitor is None or self._monitor.done():
            self._monitor = asyncio.ensure_future(self._observe())

    async def _observe(self) -> None:
        """Read the results folder every `result_interval`, and the queue every `status_interval`, while tasks wait."""
        while self.waiting or self.gathering:
            await asyncio.sleep(self.profile.result_interval)
            self._read_results()
            if self.waiting and time.monotonic() - self._last_status >= self.profile.status_interval:
                self._last_status = time.monotonic()
                await self._read_queue()

    def _read_results(self) -> None:
        try:
            names = set(os.listdir(self.results))
        except OSError:
            return
        for token, waiter in list(self.waiting.items()):
            if f"{token}.json" not in names:
                continue
            try:
                found = read(Path(waiter.entry.result))
            except ValueError as exc:
                self._end(token, Lost(None, str(exc)), End.READ)
                continue
            if found is None:
                continue
            if found.token != token:
                self._end(token, Lost(None, f"its result names another task, {found.token}"), End.READ)
                continue
            if not waiter.started:
                waiter.started = True
                waiter.hooks.started()
            self._end(token, found, End.READ)

    async def _read_queue(self) -> None:
        found = await commands.queue(self.prefix)
        if found is None:
            # squeue failed: nothing is known, so nothing is taken as gone.
            return
        now = time.monotonic()
        overdue = []
        for token, waiter in self.waiting.items():
            queued = found.get(waiter.entry.task)
            if queued is not None:
                waiter.gone = None
                if queued.state == "RUNNING" and not waiter.started:
                    waiter.started = True
                    waiter.hooks.started()
            elif waiter.gone is None:
                waiter.gone = now
            elif now - waiter.gone >= self.profile.result_wait:
                overdue.append(token)
        if not overdue:
            return
        # Read the results again first: one may have appeared since the folder was last read.
        self._read_results()
        overdue = [token for token in overdue if token in self.waiting]
        states = await commands.accounting([self.waiting[token].entry.task for token in overdue])
        for token in overdue:
            waiter = self.waiting.get(token)
            if waiter is None:
                continue
            state = None if states is None else states.get(waiter.entry.task)
            waited = f"it left the queue, and no result appeared within {self.profile.result_wait:.0f}s"
            self._end(token, Lost(state, waited if states is not None else waited + "; sacct could not say why"),
                      End.LOST)

    def _end(self, token: str, ended: Ended, how: End) -> None:
        waiter = self.waiting.pop(token)
        self.done[token] = waiter.entry
        self.ledger.ended(token, how)
        if not waiter.future.done():
            waiter.future.set_result(ended)

    # Judging an end.

    async def _state(self, ended: Ended) -> str | None:
        """Slurm's state for a task that did not succeed, when it matters: why it was killed or terminated."""
        if isinstance(ended, Lost):
            return ended.state
        if ended.outcome is Outcome.TERMINATED or _killed(ended.failure):
            entry = self.done.get(ended.token)
            tasks = [] if entry is None else [entry.task]
            # sacct may lag a task's end by a moment.
            for _ in range(3):
                states = await commands.accounting(tasks)
                if states is None:
                    return None
                state = states.get(tasks[0]) if tasks else None
                if state is None and not tasks:
                    return None
                if state not in (None, "RUNNING", "COMPLETING", "PENDING"):
                    return state
                await asyncio.sleep(1.0)
        return None

    def _retry(self, state: str | None, ended: Ended, attempt: int) -> tuple[str, bool, bool] | None:
        """Why to submit the job again, and with more memory or more time; None when it is not retried."""
        if attempt > self.profile.retries:
            return None
        if state == "OUT_OF_MEMORY" or (state is None and isinstance(ended, Result) and _killed(ended.failure)):
            return "out of memory", True, False
        if state == "TIMEOUT":
            return "at its time limit", False, True
        return None

    def _failure(self, ended: Ended, state: str | None) -> Failure:
        if isinstance(ended, Result):
            if ended.outcome is Outcome.FAILED and ended.failure is not None:
                return ended.failure
            return SlurmEnded(state, f"terminated on {ended.host} before it finished")
        return SlurmEnded(state, ended.words)


@dataclass(frozen=True)
class SlurmFailure:
    """Why an array could not be submitted."""

    words: str

    def __str__(self) -> str:
        return self.words


def _attempt_log(log: Path, attempt: int) -> Path:
    """The log of a job's `attempt`: its usual log for the first, and one beside it for each retry."""
    return log if attempt == 1 else log.with_name(f"{log.stem}.attempt-{attempt}.log")


def _killed(failure: Failure | None) -> bool:
    """Whether a command or check was killed with SIGKILL, as the kernel does to a job over its cgroup's memory."""
    return isinstance(failure, (CommandFailed, CheckFailed)) and isinstance(failure.end, KilledSignal)
