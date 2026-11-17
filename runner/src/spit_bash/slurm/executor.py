"""The Slurm executor: submits ready jobs as Slurm arrays, and keeps the queue in line with the plan.

Each job the scheduler hands it becomes a want: its identity, the resources it asks for, and the task it waits on.
One loop reads the results of the tasks waited on, often; reads the queue in one call, rarely; decides what to do
with `reconcile`; and submits the jobs free to run as arrays, within the profile's limits. Every write to the state
stays with the scheduler.
"""

from __future__ import annotations

import asyncio
import collections
import hashlib
import json
import math
import os
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

from ..execute.executor import Hooks, Ran, Task as Work
from ..execute.failure import Failure, SlurmEnded
from ..execute.job import Failed
from ..execute.options import Options
from ..model import Dag, Job
from ..schedule import ReadyQueue, size
from ..state.record import Record
from . import batch as batches
from . import claim, commands, intent
from . import result as results
from .batch import Batch, Spec
from .intent import Intent, Manifests
from .profile import Profile, Resources, megabytes, minutes
from .reconcile import Identity, Mode, Reconciled, Task, reconcile, releasable
from .result import Outcome, Result, read

# How long sbatch may keep refusing an array because a queue limit is full before its jobs fail.
BUSY_GIVE_UP_SECONDS = 3600.0
# The least time between two squeue calls a job waiting to be submitted again asks for, sooner than the status
# interval.
SNAPSHOT_GAP = 10.0
# Why a job is submitted again.
MEMORY = "out of memory"
TIME = "at its time limit"


def namespace(folder: Path) -> str:
    """Eight hex digits naming the runner folder `folder`, from its path."""
    return hashlib.sha1(str(folder).encode()).hexdigest()[:8]


def job_name(folder: Path) -> str:
    """The name every Slurm job of the runner folder `folder` takes, as `spit-1a2b3c4d`.

    It names the folder and nothing of the pipeline, so one squeue call finds every task of the folder, whichever
    controller submitted it and however the pipeline has changed since.
    """
    return f"spit-{namespace(folder)}"


def identify(snapshot: list[commands.Listed], manifests: Manifests) -> tuple[Task, ...]:
    """The queue's tasks, each with the intent its script names."""
    return tuple(Task(each.task, manifests.intent_of(each.command), each.index, each.state) for each in snapshot)


class CountQueue(ReadyQueue):
    """A ready queue that counts tasks, not cores: a job takes one place of `places`, whatever it declares."""

    def __init__(self, places: int, ranks: dict[int, float]) -> None:
        super().__init__(places, None, ranks)

    def needs(self, job: Job) -> tuple[int, int]:
        return 1, 0


@dataclass(frozen=True)
class End:
    """How a task waited on ended: its result, if one was read, and Slurm's state for it, if Slurm said."""

    result: Result | None
    state: str | None
    words: str


@dataclass
class Want:
    """A job the scheduler waits on, and the task that runs it."""

    job: Job
    identity: Identity
    work: Work
    log: Path
    memory: int
    limit: float
    resources: Resources
    hooks: Hooks
    future: asyncio.Future[End]
    # No task is submitted for it before a snapshot taken after this, as `time.monotonic`: one taken once its last
    # task had ended, or once an sbatch that never answered might have been accepted.
    not_before: float
    # The Slurm task, as `123_4`, waited on; None until one is submitted or found in the queue.
    task: str | None = None
    # Its result, once read; a result other than a success waits for Slurm to say how the task ended.
    result: Result | None = None
    # When its task was first seen ended without a result, as `time.time`.
    ended: float | None = None
    started: bool = False


@dataclass(frozen=True)
class Group:
    """What the tasks of one array share: Slurm takes one set of options for all of an array's tasks."""

    operation: str
    cores: int
    memory: int
    resources: Resources
    # The power of two of minutes its tasks' times round up to, so an array asks for at most about twice the time any
    # of its tasks needs.
    band: int
    mode: Mode


class Attempts:
    """Each job's attempt and the resources it was last given, by key, kept across runs so retries stay bounded.

    The controller alone writes it, as it alone writes the state. A job's entry goes when it succeeds; one for older
    work is ignored.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        try:
            self.entries: dict[str, list] = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            self.entries = {}

    def resume(self, job: Job, memory: int, limit: float) -> tuple[int, int, float]:
        """The job's attempt, memory and time: as last given, for the same work, else the first."""
        entry = self.entries.get(job.work.key)
        if entry is None or entry[0] != job.work.fingerprint:
            return 1, memory, limit
        return int(entry[1]), int(entry[2]), float(entry[3])

    def set(self, job: Job, attempt: int, memory: int, limit: float) -> None:
        self.entries[job.work.key] = [job.work.fingerprint, attempt, memory, limit]
        self._save()

    def done(self, job: Job) -> None:
        if self.entries.pop(job.work.key, None) is not None:
            self._save()

    def _save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(prefix=f".{self.path.name}.", dir=self.path.parent)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                json.dump(self.entries, stream)
            os.replace(temporary, self.path)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)


@dataclass
class Slurm:
    """Runs jobs as tasks of Slurm arrays, keeping the queue to what `planned` says the run does."""

    profile: Profile
    dag: Dag
    options: Options
    # The runner folder's Slurm folder: intents, results, claims and attempts.
    folder: Path
    ranks: dict[int, float]
    # Each job's time from its last run, by state key, for its `--time`.
    recorded: dict[str, float]
    # The state's records, which the scheduler updates as jobs succeed, for the stamps a job's inputs should have.
    records: dict[str, Record]
    # What the run does for each key it runs, checks included: the identity `reconcile` keeps the queue to.
    planned: dict[str, Identity]
    # The queue as the controller read it as the run started, and the claims it listed just before.
    start: tuple[Task, ...]
    held: dict[str, str]
    config: commands.Config
    # Says the controller is alive; the loop calls it each pass.
    beat: Callable[[], None] = lambda: None
    queue: CountQueue = field(init=False)
    name: str = field(init=False)
    manifests: Manifests = field(init=False)
    attempts: Attempts = field(init=False)
    wants: dict[str, Want] = field(default_factory=dict, init=False)
    # The keys a job has waited on before in this run, whose next want waits for a fresh snapshot.
    waited: set[str] = field(default_factory=set, init=False)
    # The tasks whose end a job has had, which no later want waits on again, though they linger in the queue.
    finished: set[str] = field(default_factory=set, init=False)
    # Whether a job stopped waiting before its task ended, as all do when the run is stopped.
    abandoned: bool = field(default=False, init=False)
    # The last snapshot, when it was read as `time.monotonic`, and what `reconcile` made of it.
    snapshot: tuple[Task, ...] = field(default=(), init=False)
    snapshot_at: float = field(default=float("-inf"), init=False)
    decided: Reconciled | None = field(default=None, init=False)
    sent: collections.deque[float] = field(default_factory=collections.deque, init=False)
    _last_call: float = field(default=float("-inf"), init=False)
    _loop: asyncio.Task[None] | None = field(default=None, init=False)
    _made_by: dict[str, Job] = field(default_factory=dict, init=False)
    _neighbours: dict[Path, list[str]] | None = field(default=None, init=False)
    # What sbatch said when it refused an account, partition and options, which are not tried again this run.
    _refused: dict[tuple, str] = field(default_factory=dict, init=False)
    # When sbatch first said each group's queue limit was full.
    _busy_since: dict[Group, float] = field(default_factory=dict, init=False)

    def __post_init__(self) -> None:
        self.queue = CountQueue(self.profile.jobs, self.ranks)
        self.name = job_name(self.folder.parent)
        self.manifests = Manifests(self.folder)
        self.attempts = Attempts(self.folder / "attempts.json")
        self._made_by = {path: job for job in self.dag.jobs for path in job.output_paths}
        self._decide(self.start, self.held)

    @property
    def status_interval(self) -> float:
        """How often the queue is read: the profile's, kept under half the site's MinJobAge, so that a task that
        ended between two snapshots is in the second."""
        return min(self.profile.status_interval, self.config.min_job_age / 2)

    def over(self, job: Job) -> str | None:
        return None

    @property
    def width(self) -> int:
        return self.profile.jobs

    async def run(self, dag: Dag, work: Work, log: Path, options: Options, hooks: Hooks) -> Ran:
        job = work.job
        async with self.queue.slot(job):
            hooks.admitted()
            resources = self.profile.resources_for(job)
            first_memory = job.memory if job.memory is not None else resources.memory
            first_limit = self.profile.time_for(job, self.recorded.get(job.work.key), resources)
            attempt, memory, limit = self.attempts.resume(job, first_memory, first_limit)
            while True:
                attempt_log = _attempt_log(log, attempt)
                end = await self._want(job, Mode.RUN, work, attempt_log, memory, limit, resources, hooks)
                if end.result is not None and end.result.outcome is Outcome.MADE:
                    assert end.result.finished is not None, "a result of a job that made its outputs holds them"
                    self.attempts.done(job)
                    return end.result.finished
                why = _retry(end, attempt, self.profile.retries)
                grown = None if why is None else self._grown(memory, limit, why)
                if why is None or grown is None:
                    return Failed(_failure(end), attempt_log)
                memory, limit = grown
                attempt += 1
                self.attempts.set(job, attempt, memory, limit)
                hooks.retrying(attempt, why, f"{size(memory)} of memory and {minutes(limit)} minutes")

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        async with self.queue.slot(job):
            hooks.admitted()
            resources = self.profile.resources_for(job)
            memory = job.memory if job.memory is not None else resources.memory
            limit = self.profile.time_for(job, None, resources)
            end = await self._want(job, Mode.CHECK, Work(job), log, memory, limit, resources, hooks)
            if end.result is not None and end.result.outcome is Outcome.CHECKED:
                return None
            return _failure(end)

    def stop(self) -> None:
        """Cancel every task of the folder, in one call by the folder's name, when the run leaves any: it stopped while
        jobs still waited, or a task it could have taken over is still live, its job never reached.

        The lease means no other controller owns any of them, so this takes the tasks waited on, one whose sbatch was
        accepted as the run stopped, and the ones never reached. A run that ended with nothing left calls nothing. The
        next run cancels any scancel missed, as it would any task of work it does not want.
        """
        if self._loop is not None:
            self._loop.cancel()
        unreached = self.decided is not None and any(
            task.live and task.id not in self.finished for task in self.decided.waits.values())
        if self.wants or self.abandoned or unreached:
            commands.cancel_name(self.name)
        self.wants.clear()

    # Waiting.

    async def _want(self, job: Job, mode: Mode, work: Work, log: Path, memory: int, limit: float,
                    resources: Resources, hooks: Hooks) -> End:
        """Wait for the job's task to end: one already in the queue doing this work, or one submitted for it."""
        key = job.work.key
        identity = Identity(key, job.work.fingerprint, mode)
        # A first attempt may go on the last snapshot, and wait on a task already doing its work; a retry waits for a
        # snapshot taken after its last task ended.
        first = key not in self.waited
        self.waited.add(key)
        want = Want(job, identity, work, log, memory, limit, resources, hooks,
                    asyncio.get_running_loop().create_future(), float("-inf") if first else time.monotonic())
        waiting = self.decided.waits.get(key) if first and self.decided is not None else None
        if waiting is not None and self.planned.get(key) == identity and waiting.id not in self.finished:
            want.task = waiting.id
            hooks.queued(waiting.id)
        self.wants[key] = want
        if self._loop is None or self._loop.done():
            self._loop = asyncio.ensure_future(self._run_loop())
        try:
            return await want.future
        finally:
            self.wants.pop(key, None)
            # Cancelling the wait cancels the future too: a job stopped, as the run stops, while its task may run on.
            if want.future.cancelled() or not want.future.done():
                self.abandoned = True

    async def _run_loop(self) -> None:
        while self.wants:
            await asyncio.sleep(self.profile.result_interval)
            try:
                self._read_results()
                if self._snapshot_due():
                    await self._observe()
                await self._submit_free()
            except OSError:
                # A shared filesystem's passing error, as a stale handle: the next pass tries again.
                pass
            self.beat()

    def _snapshot_due(self) -> bool:
        since = time.monotonic() - self.snapshot_at
        if since >= self.status_interval:
            return True
        # Sooner, though never more often than SNAPSHOT_GAP, when a job waits for one to be submitted again.
        return since >= min(SNAPSHOT_GAP, self.status_interval) and any(want.task is None and want.not_before > self.snapshot_at
                                             for want in self.wants.values())

    def _read_results(self) -> None:
        """Read the result of each task waited on that has written one; a success or a check passed ends its job."""
        try:
            names = set(os.listdir(results.folder(self.folder)))
        except OSError:
            return
        for want in list(self.wants.values()):
            if want.task is None or want.result is not None or f"{want.task}.json" not in names:
                continue
            path = results.path(self.folder, want.task)
            try:
                found = read(path)
            except ValueError as exc:
                self._resolve(want, End(None, None, str(exc)))
                continue
            except OSError:
                continue
            if found is None:
                continue
            path.unlink(missing_ok=True)
            if found.outcome is Outcome.BUSY:
                # Another task of the key held its claim: the next snapshot finds which, and the job waits on it.
                want.task = None
                want.not_before = time.monotonic()
                continue
            if not want.started:
                want.started = True
                want.hooks.started()
            if found.outcome in (Outcome.MADE, Outcome.CHECKED):
                (self.options.run.folder / "slurm" / "out" / f"{want.task}.out").unlink(missing_ok=True)
                self._resolve(want, End(found, None, ""))
            else:
                # Ended otherwise: Slurm's state for the task says why, for a retry, once a snapshot shows it.
                want.result = found

    async def _observe(self) -> None:
        """Read the queue, decide, and act: cancel, release claims, and settle each job whose task has ended."""
        held = claim.holders(self.folder)
        listed = await commands.snapshot(self.name)
        if listed is None:
            return
        self._decide(identify(listed, self.manifests), held)
        decided = self.decided
        assert decided is not None, "a snapshot was just decided"
        if decided.cancel:
            commands.cancel([task.id for task in decided.cancel])
        for each in releasable(held, self.snapshot):
            claim.release(self.folder, each)
        by_id = {task.id: task for task in self.snapshot}
        vanished = []
        for key, want in list(self.wants.items()):
            waited = decided.waits.get(key)
            if (waited is not None and want.task != waited.id and want.identity == self.planned.get(key)
                    and waited.id not in self.finished):
                want.task = waited.id
                want.hooks.queued(waited.id)
            if want.task is None:
                continue
            task = by_id.get(want.task)
            if task is None:
                # Not listed: submitted after the snapshot was read, or ended so long ago that Slurm forgot it.
                if want.started or want.result is not None:
                    vanished.append(want)
                continue
            if task.live:
                if task.state == "RUNNING" and not want.started:
                    want.started = True
                    want.hooks.started()
                continue
            if want.result is not None or task.state != "COMPLETED":
                self._resolve(want, End(want.result, task.state, f"it ended as {task.state}"))
            elif want.ended is None:
                want.ended = time.time()
            elif time.time() - want.ended >= self.profile.visibility:
                self._resolve(want, End(None, task.state, f"it completed, and no result appeared within "
                                                          f"{self.profile.visibility:.0f}s"))
        if vanished:
            # Gone between two snapshots, which only a MinJobAge shorter than two status intervals allows: sacct
            # says how they ended, in one call.
            states = await commands.accounting([want.task for want in vanished if want.task]) or {}
            for want in vanished:
                state = states.get(want.task or "")
                self._resolve(want, End(want.result, state, "it left the queue between two reads of it"))

    def _decide(self, snapshot: tuple[Task, ...], held: dict[str, str]) -> None:
        self.snapshot = snapshot
        self.snapshot_at = time.monotonic()
        claims = {key: held[claim.name(key)] for key in self.planned if claim.name(key) in held}
        self.decided = reconcile(self.planned, snapshot, self.manifests.identify, claims)

    def _resolve(self, want: Want, end: End) -> None:
        if want.task is not None:
            self.finished.add(want.task)
        if not want.future.done():
            want.future.set_result(end)

    # Submitting.

    async def _submit_free(self) -> None:
        """Submit, as arrays, every job with no live task in the last snapshot taken late enough to show one."""
        decided = self.decided
        if decided is None:
            return
        ready = [want for key, want in self.wants.items()
                 if want.task is None and not want.future.done() and key in decided.free
                 and self.snapshot_at >= want.not_before]
        groups: dict[Group, list[Want]] = {}
        for want in ready:
            group = Group(want.job.operation, want.job.cores, want.memory, want.resources, _band(want.limit),
                          want.identity.mode)
            groups.setdefault(group, []).append(want)
        for group, wants in sorted(groups.items(), key=lambda item: min(want.job.id for want in item[1])):
            wants.sort(key=lambda want: want.identity.key)
            for start in range(0, len(wants), self.config.max_array):
                await self._send(group, wants[start:start + self.config.max_array])

    def _expected(self, job: Job) -> tuple[tuple[str, tuple[int, ...]], ...]:
        """Each of the job's inputs that another job makes, with the stamp that job's record gives it."""
        expected = []
        for path in dict.fromkeys(job.input_paths):
            maker = self._made_by.get(path)
            record = None if maker is None else self.records.get(maker.work.key)
            found = None if maker is None or record is None else record.outputs.get(maker.identities[path])
            if found is not None:
                expected.append((path, tuple(found)))
        return tuple(expected)

    async def _send(self, group: Group, wants: list[Want]) -> None:
        """Write the array's intent, then submit it; each job then waits on its task."""
        resources = group.resources
        refused = self._refused.get(_where(resources))
        if refused is not None:
            for want in wants:
                self._resolve(want, End(None, None, f"could not submit it: {refused}"))
            return
        options = [
            f"--job-name={self.name}", f"--cpus-per-task={group.cores}", f"--mem={megabytes(group.memory)}M",
            f"--time={minutes(max(want.limit for want in wants))}", "--no-requeue",
            *([f"--account={resources.account}"] if resources.account else []),
            *([f"--partition={resources.partition}"] if resources.partition else []),
            *resources.options,
        ]
        manifest = tuple(want.identity for want in wants)
        name = intent.name(manifest, tuple(options))
        if self._neighbours is None:
            self._neighbours = batches.by_folder(self.dag)
        specs = tuple(Spec(want.job.id, want.identity.mode, want.log, want.work.reason, want.work.keep_edited,
                           self._expected(want.job)) for want in wants)
        trimmed = batches.trim(self.dag, [want.job for want in wants], self._neighbours)
        work = Batch(trimmed, self.options, specs, name, self.folder, self.profile.visibility)
        out = self.options.run.folder / "slurm" / "out"
        try:
            script = await asyncio.to_thread(intent.write, self.folder, Intent(name, manifest), work,
                                             resources.setup, self.profile.command)
            out.mkdir(parents=True, exist_ok=True)
            results.folder(self.folder).mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            # As a full disk or quota: nothing was submitted, and the jobs fail rather than wait for good.
            for want in wants:
                self._resolve(want, End(None, None, f"could not write its intent: {exc}"))
            return
        throttle = f"%{self.profile.array_limit}" if self.profile.array_limit else ""
        await self._rate()
        await self._space()
        try:
            array = await commands.submit(script, [f"--array=0-{len(wants) - 1}{throttle}", f"--comment=spit:{name}",
                                                   f"--output={out / '%A_%a.out'}", *options])
        except commands.Busy as exc:
            first = self._busy_since.setdefault(group, time.monotonic())
            if time.monotonic() - first >= BUSY_GIVE_UP_SECONDS:
                for want in wants:
                    self._resolve(want, End(None, None, f"could not submit it: {exc}"))
            return
        except commands.Unsure:
            # Perhaps accepted: the next snapshot shows its tasks, which the jobs then wait on, or does not, and they
            # are submitted again as the same intent.
            for want in wants:
                want.not_before = time.monotonic() + SNAPSHOT_GAP
            return
        except commands.SlurmError as exc:
            self._refused[_where(resources)] = str(exc)
            for want in wants:
                self._resolve(want, End(None, None, f"could not submit it: {exc}"))
            return
        self._busy_since.pop(group, None)
        for index, want in enumerate(wants):
            want.task = f"{array}_{index}"
            want.hooks.queued(want.task)

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

    async def _space(self) -> None:
        """Wait until this sbatch call comes at least `per / submissions` after the one before."""
        gap = self.profile.per / self.profile.submissions
        now = time.monotonic()
        at = max(now, self._last_call + gap)
        self._last_call = at
        if at > now:
            await asyncio.sleep(at - now)

    # Retrying.

    def _grown(self, memory: int, limit: float, why: str) -> tuple[int, float] | None:
        """The memory and time a retry asks for, within `max_memory` and `max_time`; None when neither grows."""
        grown_memory, grown_limit = memory, limit
        if why == MEMORY:
            grown_memory = int(memory * self.profile.memory_factor)
            if self.profile.max_memory is not None:
                grown_memory = min(grown_memory, self.profile.max_memory)
        else:
            grown_limit = limit * self.profile.retry_time_factor
            if self.profile.max_time is not None:
                grown_limit = min(grown_limit, self.profile.max_time)
        if grown_memory <= memory and minutes(grown_limit) <= minutes(limit):
            return None
        return grown_memory, grown_limit


def _retry(end: End, attempt: int, retries: int) -> str | None:
    """Why to submit a job again, from Slurm's state for its task: out of memory, or at its time limit."""
    if attempt > retries:
        return None
    if end.state == "OUT_OF_MEMORY":
        return MEMORY
    if end.state == "TIMEOUT":
        return TIME
    return None


def _failure(end: End) -> Failure:
    if end.result is not None and end.result.outcome is Outcome.FAILED and end.result.failure is not None:
        return end.result.failure
    if end.result is not None and end.result.outcome is Outcome.TERMINATED:
        return SlurmEnded(end.state, f"terminated on {end.result.host} before it finished")
    return SlurmEnded(end.state, end.words)


def _band(limit: float) -> int:
    """The power of two of minutes that `limit` rounds up to."""
    return 1 << max(0, math.ceil(math.log2(minutes(limit))))


def _where(resources: Resources) -> tuple:
    """What sbatch may refuse of a submission whatever its jobs: its account, partition and options."""
    return resources.account, resources.partition, resources.options


def _attempt_log(log: Path, attempt: int) -> Path:
    """The log of a job's `attempt`: its usual log for the first, and one beside it for each retry."""
    return log if attempt == 1 else log.with_name(f"{log.stem}.attempt-{attempt}.log")
