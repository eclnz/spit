"""The Slurm executor: submits ready jobs as Slurm arrays, and keeps the queue in line with the plan.

Each job the scheduler hands it becomes a want: its identity, the resources it asks for, and the task it waits on.
One loop reads the results of the tasks waited on, often; reads the queue in one call, rarely; decides what to do
with `reconcile`; and submits the jobs free to run as arrays, within the profile's limits. Every write to the state
stays with the scheduler.
"""

from __future__ import annotations

import asyncio
import hashlib
import math
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Annotated, Callable, NamedTuple

from pydantic import ConfigDict, Field, FiniteFloat, TypeAdapter

from ..execute.executor import Hooks, Ran, Task as Work
from ..execute.failure import Failure, SlurmEnded
from ..execute.job import Failed
from ..execute.options import Options
from ..model import Dag, DagError, Job
from ..schedule import ReadyQueue, size
from ..state.record import Record
from . import batch as batches
from . import claim, commands, intent
from . import result as results
from .batch import Batch, Spec
from .dispatch import Dispatches, terminal_accounting
from .intent import Intent, Manifests
from .profile import Profile, Resources, megabytes, minutes
from .reconcile import Identity, Mode, Reconciled, Task, reconcile, releasable
from .result import Outcome, Result, read
from .throttle import SubmitRate

# Quota refusals give the site a bounded pause before each retry.
BUSY_RETRIES = 3
BUSY_BACKOFF = 15.0
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


def holders(snapshot: tuple[Task, ...], identify: Callable[[Task], Identity | None],
            held: dict[str, str]) -> dict[str, str]:
    """The task holding the claim of each key's live task, by key, for `reconcile` to keep."""
    found: dict[str, str] = {}
    for task in snapshot:
        identity = identify(task) if task.live and task.intent is not None else None
        holder = None if identity is None else held.get(claim.name(task.intent or "", task.index))
        if identity is not None and holder is not None:
            found.setdefault(identity.key, holder)
    return found


def identify(snapshot: list[commands.Listed], manifests: Manifests) -> tuple[Task, ...]:
    """The queue's tasks, with ownership only for an exact manifest and array index."""
    found = []
    for row in snapshot:
        task = Task(row.task, manifests.intent_of(row.command), row.index, row.state)
        if not row.task.endswith(f"_{row.index}") or manifests.identify(task) is None:
            task = Task(row.task, None, row.index, row.state)
        found.append(task)
    return tuple(found)


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
    intent: str | None = None
    # The result and Slurm's terminal state can arrive in either order.
    result: Result | None = None
    terminal: str | None = None
    # When the controller first saw the task completed, and first read the queue without finding the task, as
    # `time.monotonic`.
    ended: float | None = None
    missing: float | None = None
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


class Attempt(NamedTuple):
    """A work fingerprint and its current retry number, memory, and time limit."""

    fingerprint: str
    number: Annotated[int, Field(ge=1)]
    memory: Annotated[int, Field(ge=0)]
    limit: Annotated[FiniteFloat, Field(ge=0)]


ATTEMPTS = TypeAdapter(dict[str, Attempt], config=ConfigDict(strict=True))


class Attempts:
    """Each job's attempt and the resources it was last given, by key, kept across runs so retries stay bounded.

    The controller alone writes it, as it alone writes the state. A job's entry goes when it succeeds; one for older
    work is ignored.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        try:
            self.entries = ATTEMPTS.validate_json(path.read_bytes())
        except FileNotFoundError:
            self.entries = {}
        except (OSError, ValueError) as exc:
            raise DagError(f"cannot verify Slurm retry history {path}: {exc}; retry allowance kept") from exc

    def resume(self, job: Job, memory: int, limit: float) -> tuple[int, int, float]:
        """The job's attempt, memory and time: as last given, for the same work, else the first."""
        entry = self.entries.get(job.work.key)
        if entry is None or entry.fingerprint != job.work.fingerprint:
            return 1, memory, limit
        return entry.number, entry.memory, entry.limit

    def set(self, job: Job, attempt: int, memory: int, limit: float) -> None:
        self.entries[job.work.key] = Attempt(job.work.fingerprint, attempt, memory, limit)
        self._save()

    def done(self, job: Job) -> None:
        if self.entries.pop(job.work.key, None) is not None:
            self._save()

    def _save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        intent.atomic(self.path, ATTEMPTS.dump_json(self.entries))


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
    # Checks the shared controller lease before changing Slurm jobs.
    check_lease: Callable[[], None] = lambda: None
    queue: CountQueue = field(init=False)
    name: str = field(init=False)
    manifests: Manifests = field(init=False)
    dispatches: Dispatches = field(init=False)
    throttle: SubmitRate = field(init=False)
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
    # The gap before the next read while nothing is waiting on one, and the task states the last read found.
    gap: float = field(default=0.0, init=False)
    _seen: frozenset[tuple[str, str]] = field(default=frozenset(), init=False)
    decided: Reconciled | None = field(default=None, init=False)
    _halted: str | None = field(default=None, init=False)
    _cancel_requested: set[str] = field(default_factory=set, init=False)
    # Successful results are recoverable until their state record is written.
    _awaiting_record: dict[str, Result] = field(default_factory=dict, init=False)
    _loop: asyncio.Task[None] | None = field(default=None, init=False)
    _made_by: dict[str, Job] = field(default_factory=dict, init=False)
    _neighbours: dict[Path, list[str]] | None = field(default=None, init=False)
    # What sbatch said when it refused an account, partition and options, which are not tried again this run.
    _refused: dict[tuple, str] = field(default_factory=dict, init=False)
    # Quota refusals and the next allowed retry, by request.
    _busy: dict[Group, tuple[int, float]] = field(default_factory=dict, init=False)
    # How many intents the run has written, which names each.
    _submissions: int = field(default=0, init=False)

    def __post_init__(self) -> None:
        if self.config.max_array < 1:
            raise DagError("Slurm's MaxArraySize disables array submissions")
        self.queue = CountQueue(self.profile.jobs, self.ranks)
        self.name = job_name(self.folder.parent)
        self.manifests = Manifests(self.folder)
        self.dispatches = Dispatches(self.folder)
        self.throttle = SubmitRate(self.folder, self.profile)
        self.attempts = Attempts(self.folder / "attempts.json")
        self.gap = self.status_interval
        self._made_by = {path: job for job in self.dag.jobs for path in job.output_paths}
        self._decide(self.start, self.held)

    @property
    def ceiling(self) -> float:
        """The longest gap between queue reads: the profile's `max_status_interval`, kept under half the site's
        MinJobAge, so that a task that ended between two reads is in the second.

        Keep in step with `behaviour.limits`, which reports both gaps.
        """
        longest = max(self.profile.max_status_interval, self.profile.status_interval)
        return min(longest, max(1.0, self.config.min_job_age / 2))

    @property
    def status_interval(self) -> float:
        """The gap between queue reads while tasks change: the profile's, within `ceiling`."""
        return min(self.profile.status_interval, self.ceiling)

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
        """On interruption, request cancellation of tasks the current queue proves belong to this folder."""
        if self._loop is not None:
            self._loop.cancel()
        self._retire_results(best_effort=True)
        unreached = self.decided is not None and any(
            task.live and task.id not in self.finished for task in self.decided.waits.values())
        if self._halted is None and (self.abandoned or any(not want.future.done() for want in self.wants.values())
                                     or unreached):
            try:
                self.check_lease()
            except DagError as exc:
                print(f"error: {exc}; Slurm reservations kept", file=sys.stderr)
                return
            listed = commands.snapshot_now(self.name)
            if listed is None:
                print("error: cannot verify Slurm cancellation targets; reservations kept", file=sys.stderr)
            else:
                tasks = identify(listed, self.manifests)
                try:
                    self.dispatches.reconcile(tasks)
                except OSError as exc:
                    print(f"error: could not update Slurm reservations: {exc}", file=sys.stderr)
                owned = sorted({task.id for task in tasks if task.live and task.intent is not None})
                failed = commands.cancel(owned)
                if owned:
                    print(f"note: requested Slurm cancellation for {', '.join(owned)}; terminal status unconfirmed",
                          file=sys.stderr)
                if failed:
                    print(f"error: could not request cancellation for {', '.join(failed)}", file=sys.stderr)
                if self.dispatches.unknown:
                    print(f"error: Slurm acceptance unresolved for {', '.join(self.dispatches.unknown)}",
                          file=sys.stderr)
        self.wants.clear()

    # Waiting.

    async def _want(self, job: Job, mode: Mode, work: Work, log: Path, memory: int, limit: float,
                    resources: Resources, hooks: Hooks) -> End:
        """Wait for the job's task to end: one already in the queue doing this work, or one submitted for it."""
        if self._halted:
            return End(None, None, self._halted)
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
            want.intent = waiting.intent
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
        while self.wants and self._halted is None:
            await asyncio.sleep(self.profile.result_interval)
            try:
                self._retire_results()
                self._read_results()
                if self._snapshot_due():
                    await self._observe()
                self._expire()
                await self._submit_free()
            except (OSError, DagError) as exc:
                self._halt(f"cannot maintain Slurm reservations: {exc}")

    def _snapshot_due(self) -> bool:
        if self._halted:
            return False
        # A result whose task Slurm has yet to show ended, or a job that needs a newer read before it is submitted,
        # is a change on its way: the next read comes after `status_interval`, not after the backed-off gap.
        waiting = any(not want.future.done() and (want.result is not None and want.terminal is None
                                                  or want.task is None and want.not_before > self.snapshot_at)
                      for want in self.wants.values())
        return time.monotonic() - self.snapshot_at >= (self.status_interval if waiting else self.gap)

    def _read_results(self) -> None:
        """Read results while their tasks retain slots until Slurm confirms release."""
        try:
            names = set(os.listdir(results.folder(self.folder)))
        except OSError:
            return
        for want in list(self.wants.values()):
            if want.future.done() or want.task is None or want.result is not None or f"{want.task}.json" not in names:
                continue
            path = results.path(self.folder, want.task)
            try:
                found = read(path)
            except ValueError as exc:
                self._halt(f"cannot read the result of Slurm task {want.task}: {exc}")
                return
            except OSError:
                continue
            if found is None:
                continue
            if (found.task, found.intent, found.key, found.fingerprint) != (
                    want.task, want.intent, want.identity.key, want.identity.fingerprint):
                self._halt(f"result for Slurm task {want.task} does not identify the work being tracked")
                return
            if not want.started:
                want.started = True
                want.hooks.started()
            want.result = found
            self._complete(want)

    async def _observe(self) -> None:
        """Read the queue and release capacity only using proven terminal states."""
        held = claim.holders(self.folder)
        tracked = self.dispatches.tasks().keys() | {task.id for task in self.snapshot if task.live}
        listed = await commands.snapshot(self.name, tuple(sorted(tracked)))
        if listed is None:
            self._halt("cannot read the Slurm queue; submission stopped and outstanding reservations kept")
            return
        visible = identify(listed, self.manifests)
        self.dispatches.reconcile(visible)
        ended = await terminal_accounting(self.dispatches.missing(visible))
        self.dispatches.reconcile(ended)
        self._decide(visible + ended, held)
        seen = frozenset((task.id, task.state) for task in visible + ended)
        self.gap = self.status_interval if seen != self._seen else min(self.gap * 2, self.ceiling)
        self._seen = seen
        decided = self.decided
        assert decided is not None, "a snapshot was just decided"
        seen_owned = {(task.id, task.intent) for task in visible if task.intent is not None and task.live}
        targets = [task.id for task in decided.cancel
                   if (task.id, task.intent) in seen_owned and task.id not in self._cancel_requested]
        if targets:
            self.check_lease()
            self._cancel_requested.update(targets)
            failed = commands.cancel(targets)
            if failed:
                self._halt(f"could not request cancellation for {', '.join(failed)}; reservations kept")
                return
        for each in releasable(held, self.snapshot):
            claim.release(self.folder, each)
        by_id = {task.id: task for task in self.snapshot if task.intent is not None}
        now = time.monotonic()
        for key, want in list(self.wants.items()):
            if want.future.done():
                continue
            waited = decided.waits.get(key)
            if (waited is not None and want.task is None and want.identity == self.planned.get(key)
                    and waited.id not in self.finished):
                want.task = waited.id
                want.intent = waited.intent
                want.hooks.queued(waited.id)
            if want.task is None:
                continue
            task = by_id.get(want.task)
            if task is None or task.intent != want.intent:
                if want.terminal is not None:
                    continue
                if want.missing is None:
                    want.missing = now
                # Judged at a read, not on a timer, so a gap between reads longer than `visibility` cannot end the
                # wait before the queue and accounting are asked again.
                elif now - want.missing >= self.profile.visibility:
                    self._halt(f"terminal state of Slurm task {want.task} could not be verified; reservation kept")
                    return
                continue
            want.missing = None
            if task.live:
                if task.state == "RUNNING" and not want.started:
                    want.started = True
                    want.hooks.started()
                continue
            want.terminal = task.state
            if want.ended is None:
                want.ended = now
            self._complete(want)

    def _decide(self, snapshot: tuple[Task, ...], held: dict[str, str]) -> None:
        self.snapshot = snapshot
        self.snapshot_at = time.monotonic()
        present = {(task.id, task.intent) for task in snapshot}
        absent = tuple(task for task in self.dispatches.tasks().values() if (task.id, task.intent) not in present)
        candidates = snapshot + absent
        self.decided = reconcile(self.planned, candidates, self.manifests.identify,
                                 holders(candidates, self.manifests.identify, held))

    def _retire_results(self, *, best_effort: bool = False) -> None:
        for task, result in list(self._awaiting_record.items()):
            record = self.records.get(result.key)
            if record is not None and (record.run, record.fingerprint) == (result.run, result.fingerprint):
                try:
                    results.path(self.folder, task).unlink(missing_ok=True)
                except OSError:
                    if best_effort:
                        continue
                    raise
                del self._awaiting_record[task]

    def _complete(self, want: Want) -> None:
        if want.terminal is None or (want.result is None and want.terminal == "COMPLETED"):
            return
        if want.result is not None and want.result.outcome is Outcome.BUSY:
            self._halt(f"Slurm task {want.task} could not take its claim; submission stopped")
            return
        if want.result is not None and want.result.outcome in (Outcome.MADE, Outcome.CHECKED):
            (self.options.run.folder / "slurm" / "out" / f"{want.task}.out").unlink(missing_ok=True)
        if want.result is not None and want.result.outcome is Outcome.MADE:
            self._awaiting_record[want.result.task] = want.result
        elif want.task is not None:
            results.path(self.folder, want.task).unlink(missing_ok=True)
        self._resolve(want, End(want.result, want.terminal, f"it ended as {want.terminal}"))

    def _expire(self) -> None:
        now = time.monotonic()
        for want in self.wants.values():
            if want.future.done():
                continue
            if want.ended is not None and now - want.ended >= self.profile.visibility:
                self._resolve(want, End(None, want.terminal, f"it completed, and no result appeared within "
                                                             f"{self.profile.visibility:.0f}s"))

    def _halt(self, detail: str) -> None:
        """Stop submitting and report which reservations still need to be resolved."""
        self._halted = detail
        outstanding = sorted(self.dispatches.tasks())
        if outstanding:
            detail += f"; outstanding Slurm tasks: {', '.join(outstanding)}"
        for want in self.wants.values():
            self._resolve(want, End(None, None, detail))

    def _resolve(self, want: Want, end: End) -> None:
        if want.task is not None:
            self.finished.add(want.task)
        if not want.future.done():
            want.future.set_result(end)

    # Submitting.

    async def _submit_free(self) -> None:
        """Submit ready work only into capacity free of live or unresolved Slurm tasks."""
        decided = self.decided
        if decided is None or self._halted or self.dispatches.unknown:
            return
        ready = [want for key, want in self.wants.items()
                 if want.task is None and not want.future.done() and key in decided.free
                 and self.snapshot_at >= want.not_before]
        groups: dict[Group, list[Want]] = {}
        for want in ready:
            group = Group(want.job.operation, want.job.cores, want.memory, want.resources, _band(want.limit),
                          want.identity.mode)
            groups.setdefault(group, []).append(want)
        live = {task.id for task in self.snapshot if task.live}
        for group, wants in sorted(groups.items(), key=lambda item: min(want.job.id for want in item[1])):
            wants.sort(key=lambda want: want.identity.key)
            if group in self._busy and time.monotonic() < self._busy[group][1]:
                continue
            start = 0
            while start < len(wants) and self._halted is None:
                available = self.profile.jobs - len(live | self.dispatches.tasks().keys())
                size = min(available, self.config.max_array)
                if size <= 0:
                    return
                await self._send(group, wants[start:start + size])
                if group in self._busy:
                    break
                start += size

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
        self._submissions += 1
        name = intent.name(manifest, tuple(options), f"{self.options.run.name}-{self._submissions}")
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
        arguments = [f"--array=0-{len(wants) - 1}{throttle}", f"--comment=spit:{name}",
                     f"--output={out / '%A_%a.out'}", *options]
        try:
            array = await self._submit(script, arguments, name, len(wants))
        except commands.Unsure as exc:
            self._halt(f"submission of intent {name} is unconfirmed: {exc}; see {self.folder / (name + '.dispatch')}")
            return
        except commands.Busy as exc:
            self.dispatches.clear(name)
            intent.remove(self.folder, name)
            attempts = self._busy.get(group, (0, 0))[0] + 1
            self._busy[group] = (attempts, time.monotonic() + BUSY_BACKOFF * attempts)
            if attempts > BUSY_RETRIES:
                self._refused[_where(resources)] = str(exc)
                for want in wants:
                    self._resolve(want, End(None, None, f"Slurm quota still full after bounded retries: {exc}"))
            return
        except commands.SlurmError as exc:
            self.dispatches.clear(name)
            intent.remove(self.folder, name)
            self._refused[_where(resources)] = str(exc)
            for want in wants:
                self._resolve(want, End(None, None, f"could not submit it: {exc}"))
            return
        self.dispatches.accept(name, array)
        self._busy.pop(group, None)
        for index, want in enumerate(wants):
            want.task = f"{array}_{index}"
            want.intent = name
            want.hooks.queued(want.task)

    async def _submit(self, script: Path, arguments: list[str], name: str, slots: int) -> str:
        """Make one submission attempt for this intent; an unknown reply keeps its reservation."""
        await self.throttle.wait()
        self.check_lease()
        self.dispatches.reserve(name, slots)
        self.throttle.charge()
        try:
            return await commands.submit(script, arguments)
        finally:
            self.throttle.settled()

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
