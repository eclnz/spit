"""The Slurm executor: submits ready jobs as Slurm arrays, and learns how each ended from its result file.

A job's task runs `spit-bash job` on a compute node, which writes the result; the controller reads the results
folder often and the queue rarely, in one `squeue` call for the whole run, and asks `sacct` only why a task that
left the queue without a result ended. Every write to the state stays with the scheduler, on the controller.
"""

from __future__ import annotations

import asyncio
import collections
import hashlib
import math
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
# How long questions for sacct gather before one call answers them all, and how many rounds a task still listed as
# running gets, as sacct may lag a task's end.
ACCOUNTING_GATHER = 2.0
ACCOUNTING_ROUNDS = 3
# The most job names a squeue call filters by; a run with more operations filters by prefix alone.
NAMES_SHOWN = 200


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
    # The power of two of minutes its tasks' times round up to, so that an array asks for at most about twice the
    # time any of its tasks needs, rather than every task asking for the longest's.
    band: int


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
    # When the last sbatch call was made, or is reserved to be, as `time.monotonic` gives it.
    _last_call: float = field(default=float("-inf"), init=False)
    # The job that makes each output path.
    _made_by: dict[str, Job] = field(default_factory=dict, init=False)
    # Each task whose end was dealt with, by token, for asking sacct why it ended.
    done: dict[str, Entry] = field(default_factory=dict, init=False)
    # The run's job names, for squeue to filter by.
    names: tuple[str, ...] = field(default=(), init=False)
    # The last reading of the queue, with when it was made, which every reader shares.
    _queue_read: tuple[float, dict[str, commands.Queued] | None] | None = field(default=None, init=False)
    _queue_lock: asyncio.Lock = field(default_factory=asyncio.Lock, init=False)
    # Tasks waiting for sacct to say how they ended, and what answers them.
    _asking: dict[str, list[asyncio.Future[tuple[bool, str | None]]]] = field(default_factory=dict, init=False)
    _asker: asyncio.Task[None] | None = field(default=None, init=False)
    # Each batch's files and how many of its tasks are still waited on, by batch token; and each task's batch.
    _batches: dict[str, tuple[int, tuple[Path, ...]]] = field(default_factory=dict, init=False)
    _batch_of: dict[str, str] = field(default_factory=dict, init=False)
    # What sbatch said when it refused an account, partition and options, which are not tried again this run.
    _refused: dict[tuple[str | None, str | None, tuple[str, ...]], str] = field(default_factory=dict, init=False)

    def __post_init__(self) -> None:
        self.queue = CountQueue(self.profile.jobs, self.ranks)
        self.prefix = f"spit-{namespace(self.folder.parent)}-"
        self._made_by = {path: job for job in self.dag.jobs for path in job.output_paths}
        self.names = job_names(self.prefix, self.dag)

    @property
    def files(self) -> Path:
        """This run's Slurm files, in its run folder, so pruning the run removes them: batches, Slurm's output for
        each task, and the results not read yet."""
        return self.options.run.folder / "slurm"

    @property
    def results(self) -> Path:
        return self.files / "results"

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
                grown = self._grown(memory, limit, more_memory, more_time)
                if grown is None:
                    # At the profile's ceiling: another attempt would ask for what the last one had.
                    return Failed(self._failure(ended, state), attempt_log)
                memory, limit = grown
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
        """Cancel every task still queued or running, and every task taken over from an earlier run that this run
        did not reach, as a run that ends or is stopped does.

        Their ledger entries stay pending, whether scancel worked or not: a cancelled task can take a while to stop,
        and putting back its job's earlier outputs as it does, so the next run looks in the queue and asks sacct how
        each ended rather than trusting that it has.
        """
        for task in (self._monitor, self._asker):
            if task is not None:
                task.cancel()
        tasks = [waiter.entry.task for waiter in self.waiting.values()]
        tasks += [entry.task for entry in self.adopted.values()]
        if tasks:
            commands.cancel(tasks)
        self.waiting.clear()
        self.adopted.clear()
        self.ledger.flush()

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
        group = Group(job.operation, job.cores, memory, resources, _band(limit))
        gathering = self.gathering.setdefault(group, [])
        gathering.append(Pending(spec, job, limit, future, hooks))
        if len(gathering) == 1:
            asyncio.get_running_loop().call_later(self.profile.gather, lambda: asyncio.ensure_future(self._flush(group)))
        self._watch()
        return await future

    async def _flush(self, group: Group) -> None:
        """Submit a group's gathered jobs, taking them only once an sbatch call is allowed: jobs that became ready
        while it waited for the submission rate join the same array, so arrays grow as submissions back up, rather
        than each job of a chain becoming an sbatch call of its own."""
        pending: list[Pending] = []
        try:
            if self._max_array is None:
                self._max_array = await asyncio.to_thread(commands.max_array_size) or 1001
            await self._rate()
            pending = [each for each in self.gathering.pop(group, []) if not each.future.done()]
            for start in range(0, len(pending), self._max_array):
                await self._send(group, pending[start:start + self._max_array], paid=start == 0)
        except Exception as exc:
            # As a full disk or quota leaves a batch unwritten: the jobs fail, rather than wait for good.
            pending = pending or self.gathering.pop(group, [])
            for each in pending:
                if not each.future.done():
                    each.future.set_result(Lost(None, f"could not submit it: {exc}"))

    async def _rate(self) -> None:
        """Wait until another sbatch call keeps within the profile's submission rate, with the calls spaced evenly
        through it, so that many arrays ready at once reach slurmctld one by one rather than in a burst."""
        gap = self.profile.per / self.profile.submissions
        while True:
            now = time.monotonic()
            while self.sent and now - self.sent[0] >= self.profile.per:
                self.sent.popleft()
            if len(self.sent) < self.profile.submissions and (not self.sent or now - self.sent[-1] >= gap):
                self.sent.append(now)
                return
            wait = gap - (now - self.sent[-1]) if self.sent else gap
            if len(self.sent) >= self.profile.submissions:
                wait = max(wait, self.profile.per - (now - self.sent[0]))
            await asyncio.sleep(max(wait, 0.01))

    async def _space(self) -> None:
        """Wait until this sbatch call comes at least `per / submissions` after the one before.

        `_rate` grants a call before its batch is written, which takes a varying time, so two calls granted a gap
        apart could still reach slurmctld together; this spaces the calls themselves. Each reserves its moment at
        once, so calls made together queue one gap apart.
        """
        gap = self.profile.per / self.profile.submissions
        now = time.monotonic()
        at = max(now, self._last_call + gap)
        self._last_call = at
        if at > now:
            await asyncio.sleep(at - now)

    async def _send(self, group: Group, pending: list[Pending], paid: bool = False) -> None:
        """Submit `pending` as one array, record each task in the ledger, and wait on it. With `paid`, the first
        sbatch call is already within the submission rate."""
        refused = self._refused.get(_where(group.resources))
        if refused is not None:
            # sbatch refused these options already, as for an account or partition that does not exist: asking
            # again, array after array, would only load slurmctld.
            for each in pending:
                if not each.future.done():
                    each.future.set_result(Lost(None, f"could not submit it: {refused}"))
            return
        run = self.options.run.name
        token = secrets.token_hex(6)
        place = self.files / "batches"
        if self._neighbours is None:
            self._neighbours = batches.by_folder(self.dag)
        trimmed = batches.trim(self.dag, [each.job for each in pending], self._neighbours)
        longest = max(each.time for each in pending)
        # A task waiting for its inputs holds its node idle, so it waits at most a quarter of the shortest task's time.
        wait = min(self.profile.result_wait, min(each.time for each in pending) / 4)
        await asyncio.to_thread(batches.write, Batch(trimmed, self.options, tuple(each.spec for each in pending), wait),
                                place / f"{token}.pickle")
        script = place / f"{token}.sh"
        command = shlex.join([*self.profile.command, "job", str(place / f"{token}.pickle")])
        # SIGTERM is ignored from here until `spit-bash job` installs its own handler, which an ignored signal
        # carries across `exec` to: a time-limit warning that came while Python started would otherwise end the task
        # with no result. One that comes then is lost, and the task finishes or reaches its limit.
        script.write_text("#!/bin/bash\n" + "".join(f"{line}\n" for line in group.resources.setup)
                          + "trap '' TERM\n" + f'exec {command} "$SLURM_ARRAY_TASK_ID"\n', encoding="utf-8")
        (self.files / "out").mkdir(parents=True, exist_ok=True)
        self.results.mkdir(parents=True, exist_ok=True)
        resources = group.resources
        limit = f"%{self.profile.array_limit}" if self.profile.array_limit else ""
        options = [
            f"--array=0-{len(pending) - 1}{limit}", f"--job-name={self.prefix}{group.operation}",
            f"--comment=spit:{token}", f"--cpus-per-task={group.cores}", f"--mem={megabytes(group.memory)}M",
            f"--time={minutes(longest)}",
            f"--output={self.files / 'out' / '%A_%a.out'}", "--no-requeue",
            *_signal(self.profile.grace, longest),
            *([f"--account={resources.account}"] if resources.account else []),
            *([f"--partition={resources.partition}"] if resources.partition else []),
            *resources.options,
        ]
        array = await self._sbatch(script, options, token, group.resources, paid)
        self._batches[token] = (len(pending), (script, place / f"{token}.pickle"))
        if not isinstance(array, str):
            self._forget_batch(token, len(pending))
            for each in pending:
                if not each.future.done():
                    each.future.set_result(Lost(None, f"could not submit it: {array}"))
            return
        entries = [Entry(each.spec.token, f"{array}_{index}", each.job.id, each.job.work.key,
                         each.job.work.fingerprint, each.spec.attempt, str(each.spec.result), run, time.time())
                   for index, each in enumerate(pending)]
        try:
            self.ledger.submitted(entries)
        except OSError:
            # An array the ledger does not hold would run unseen, and its jobs be submitted again by the next run.
            commands.cancel([array])
            raise
        for entry, each in zip(entries, pending):
            self._batch_of[entry.token] = token
            self.waiting[entry.token] = Waiter(entry, each.future, each.hooks)
            each.hooks.queued(entry.task)
        # The monitor stops when nothing waits, which may have been while this array was being submitted.
        self._watch()

    async def _sbatch(self, script: Path, options: list[str], token: str, resources: Resources,
                      paid: bool = False) -> str | SlurmFailure:
        """Submit, waiting while the queue is full, and finding a submission sbatch may have made before failing.

        A submission sbatch never confirmed is looked for by its comment once more before it is given up on, and
        cancelled if it has appeared; one that appears later still is cancelled by the next run, which cancels every
        task of its prefix the ledger does not hold.
        """
        tries = 0
        while True:
            if not paid:
                await self._rate()
            paid = False
            await self._space()
            try:
                return await commands.submit(script, options)
            except commands.Busy:
                await asyncio.sleep(self.profile.status_interval)
            except commands.Unsure as exc:
                found = await self._queue(fresh=5.0)
                for queued in (found or {}).values():
                    if queued.comment == f"spit:{token}":
                        return queued.task.split("_")[0]
                tries += 1
                if tries >= UNSURE_TRIES:
                    await asyncio.sleep(min(self.profile.status_interval, 10.0))
                    found = await self._queue(fresh=0.0)
                    late = [queued.task for queued in (found or {}).values() if queued.comment == f"spit:{token}"]
                    if late:
                        commands.cancel(sorted({task.split("_")[0] for task in late}))
                    return SlurmFailure(str(exc))
                await asyncio.sleep(min(self.profile.status_interval, 2.0 ** tries))
            except commands.SlurmError as exc:
                self._refused[_where(resources)] = str(exc)
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
            await self._gone(entry.task)
            self.ledger.ended(entry.token, End.CANCELLED)
            return None
        future: asyncio.Future[Ended] = asyncio.get_running_loop().create_future()
        self.waiting[entry.token] = Waiter(entry, future, hooks)
        hooks.queued(entry.task)
        self._watch()
        return entry.attempt, await future

    async def _gone(self, task: str) -> None:
        """Wait until `task` has left the queue, so that it no longer writes the job's outputs."""
        while True:
            found = await self._queue(fresh=min(self.profile.status_interval, 10.0))
            if found is not None and task not in found:
                return
            await asyncio.sleep(min(self.profile.status_interval, 10.0))

    async def _queue(self, fresh: float) -> dict[str, commands.Queued] | None:
        """The run's tasks in the queue, from a squeue call made at most `fresh` seconds ago.

        Every reader shares one reading, and readers that ask at once share one call, so the run calls squeue no
        more often than its most eager reader allows, however many jobs wait.
        """
        async with self._queue_lock:
            if self._queue_read is not None and time.monotonic() - self._queue_read[0] < fresh:
                return self._queue_read[1]
            found = await commands.queue(self.prefix, self.names)
            self._queue_read = (time.monotonic(), found)
            return found

    # Watching.

    def _watch(self) -> None:
        if self._monitor is None or self._monitor.done():
            self._monitor = asyncio.ensure_future(self._observe())
            self._monitor.add_done_callback(self._watched)

    def _watched(self, monitor: asyncio.Task[None]) -> None:
        """Fail every waiting job if the monitor stopped with an error, rather than leave them waiting for good."""
        if monitor.cancelled() or monitor.exception() is None:
            return
        words = f"the controller stopped watching its tasks: {monitor.exception()}"
        for token in list(self.waiting):
            self._end(token, Lost(None, words), End.LOST)

    async def _observe(self) -> None:
        """Read the results folder every `result_interval`, and the queue every `status_interval`, while tasks wait."""
        while self.waiting or self.gathering:
            await asyncio.sleep(self.profile.result_interval)
            try:
                self._read_results()
                if self.waiting and time.monotonic() - self._last_status >= self.profile.status_interval:
                    self._last_status = time.monotonic()
                    await self._read_queue()
            except OSError:
                # A shared filesystem's passing error, as a stale handle: the next pass tries again.
                continue
            finally:
                self.ledger.flush()

    def _read_results(self) -> None:
        # A task taken over from an earlier run writes its result in that run's folder.
        names: dict[Path, set[str]] = {}
        for waiter in self.waiting.values():
            folder = Path(waiter.entry.result).parent
            if folder not in names:
                try:
                    names[folder] = set(os.listdir(folder))
                except OSError:
                    names[folder] = set()
        for token, waiter in list(self.waiting.items()):
            if f"{token}.json" not in names[Path(waiter.entry.result).parent]:
                continue
            try:
                found = read(Path(waiter.entry.result))
            except ValueError as exc:
                self._end(token, Lost(None, str(exc)), End.READ)
                continue
            except OSError:
                # Read again next pass; one that never reads is given up on once its task has left the queue.
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
            # Read once, and kept in the ledger's terms: the results folder holds only those still to read.
            result = Path(waiter.entry.result)
            result.unlink(missing_ok=True)
            if found.outcome in (Outcome.MADE, Outcome.CHECKED):
                # Slurm's output of a task that succeeded holds nothing the job's own log does not.
                (result.parent.parent / "out" / f"{waiter.entry.task}.out").unlink(missing_ok=True)

    async def _read_queue(self) -> None:
        found = await self._queue(fresh=self.profile.status_interval / 2)
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
        batch = self._batch_of.pop(token, None)
        if batch is not None:
            self._forget_batch(batch, 1)
        if not waiter.future.done():
            waiter.future.set_result(ended)

    def _forget_batch(self, token: str, ended: int) -> None:
        """Count `ended` more of a batch's tasks as ended, and delete its files once none is left to start."""
        left, paths = self._batches.get(token, (0, ()))
        if left - ended > 0:
            self._batches[token] = (left - ended, paths)
            return
        self._batches.pop(token, None)
        for path in paths:
            path.unlink(missing_ok=True)

    # Judging an end.

    async def _state(self, ended: Ended) -> str | None:
        """Slurm's state for a task that did not succeed, when it matters: why it was killed or terminated."""
        if isinstance(ended, Lost):
            return ended.state
        entry = self.done.get(ended.token)
        if entry is None or not (ended.outcome is Outcome.TERMINATED or _killed(ended.failure)):
            return None
        _, state = await self._ask(entry.task)
        if ended.outcome is Outcome.TERMINATED and state == "COMPLETED":
            # The job put its outputs back on the SIGTERM that `--signal` sends before the time limit, and exited
            # cleanly, so sacct calls it completed. A task cancelled with scancel is CANCELLED whatever it exits with.
            return "TIMEOUT"
        return state

    async def _ask(self, task: str) -> tuple[bool, str | None]:
        """Whether sacct answered, and the task's state; questions that come within `ACCOUNTING_GATHER` of each
        other share one sacct call, so a hundred tasks killed at their time limit cost one call, not a hundred."""
        future: asyncio.Future[tuple[bool, str | None]] = asyncio.get_running_loop().create_future()
        self._asking.setdefault(task, []).append(future)
        if self._asker is None or self._asker.done():
            self._asker = asyncio.ensure_future(self._answer())
        return await future

    async def _answer(self) -> None:
        try:
            await self._answer_all()
        except Exception:
            # sacct's answers only say why a task ended: without them, jobs fail with less said, but do not hang.
            asked, self._asking = self._asking, {}
            for futures in asked.values():
                for future in futures:
                    if not future.done():
                        future.set_result((False, None))
            raise

    async def _answer_all(self) -> None:
        rounds: dict[str, int] = {}
        while self._asking:
            await asyncio.sleep(ACCOUNTING_GATHER)
            asked, self._asking = self._asking, {}
            states = await commands.accounting(sorted(asked))
            for task, futures in asked.items():
                state = None if states is None else states.get(task)
                rounds[task] = rounds.get(task, 0) + 1
                # sacct may list a task as running for a moment after it ended.
                if states is not None and state in (None, "RUNNING", "COMPLETING", "PENDING") \
                        and rounds[task] < ACCOUNTING_ROUNDS:
                    self._asking.setdefault(task, []).extend(futures)
                    continue
                for future in futures:
                    if not future.done():
                        future.set_result((states is not None, state))

    def _retry(self, state: str | None, ended: Ended, attempt: int) -> tuple[str, bool, bool] | None:
        """Why to submit the job again, and with more memory or more time; None when it is not retried."""
        if attempt > self.profile.retries:
            return None
        if state == "OUT_OF_MEMORY" or (state is None and isinstance(ended, Result) and _killed(ended.failure)):
            return "out of memory", True, False
        if state == "TIMEOUT":
            return "at its time limit", False, True
        return None

    def _grown(self, memory: int, limit: float, more_memory: bool, more_time: bool) -> tuple[int, float] | None:
        """The memory and time a retry asks for, within `max_memory` and `max_time`; None when neither grows."""
        grown_memory, grown_limit = memory, limit
        if more_memory:
            grown_memory = int(memory * self.profile.memory_factor)
            if self.profile.max_memory is not None:
                grown_memory = min(grown_memory, self.profile.max_memory)
        if more_time:
            grown_limit = limit * self.profile.retry_time_factor
            if self.profile.max_time is not None:
                grown_limit = min(grown_limit, self.profile.max_time)
        if grown_memory <= memory and minutes(grown_limit) <= minutes(limit):
            return None
        return grown_memory, grown_limit

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


def job_names(prefix: str, dag: Dag) -> tuple[str, ...]:
    """The names the DAG's Slurm jobs take, for squeue to filter by; none, so squeue filters by user alone, when
    there are more than `NAMES_SHOWN`."""
    operations = sorted({job.operation for job in dag.jobs})
    return tuple(f"{prefix}{operation}" for operation in operations) if len(operations) <= NAMES_SHOWN else ()


def _band(limit: float) -> int:
    """The power of two of minutes that `limit` rounds up to."""
    return 1 << max(0, math.ceil(math.log2(minutes(limit))))


def _signal(grace: float, limit: float) -> list[str]:
    """`--signal` for SIGTERM `grace` seconds before a task's time limit, but at most a quarter of `limit` and at
    least 30 seconds, so a short task is not stopped as it starts; none with no grace."""
    if grace <= 0:
        return []
    lead = int(min(grace, max(30.0, limit / 4), minutes(limit) * 60 - 1))
    return [f"--signal=B:TERM@{lead}"]


def _where(resources: Resources) -> tuple[str | None, str | None, tuple[str, ...]]:
    """What sbatch may refuse of a submission whatever its jobs: its account, partition and options."""
    return resources.account, resources.partition, resources.options


def _attempt_log(log: Path, attempt: int) -> Path:
    """The log of a job's `attempt`: its usual log for the first, and one beside it for each retry."""
    return log if attempt == 1 else log.with_name(f"{log.stem}.attempt-{attempt}.log")


def _killed(failure: Failure | None) -> bool:
    """Whether a command or check was killed with SIGKILL, as the kernel does to a job over its cgroup's memory."""
    return isinstance(failure, (CommandFailed, CheckFailed)) and isinstance(failure.end, KilledSignal)
