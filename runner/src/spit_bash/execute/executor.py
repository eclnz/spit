"""What starts a ready job and says how it ended: `Local` runs it here, within the run's cores and memory.

The scheduler decides which jobs run and in what order, and writes the state; an executor only runs what it is
given. `slurm.executor.Slurm` is the other executor, and `Mixed` sends each job to one of two.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Protocol, Union

from ..files import hashes
from ..model import Dag, Job
from ..reuse.evidence import Compare
from ..schedule import ReadyQueue, too_big
from .failure import Failure
from .job import Failed, Made, check_job, run_job
from .options import Options

# Files this small, all of a job's together, are hashed on the event loop.
INLINE_HASH_BYTES = 1 << 20


@dataclass(frozen=True)
class Hooks:
    """What the scheduler hears of one job as an executor runs it."""

    # The job has its cores and memory, or its place in a queue, and is about to start.
    admitted: Callable[[], None] = lambda: None
    # Its inputs are there and its first check or command is about to start.
    started: Callable[[], None] = lambda: None
    # Its commands succeeded and its files are large enough to hash off the event loop.
    hashing: Callable[[], None] = lambda: None
    # It failed or was cancelled and kept its failed outputs, so the earlier record no longer describes them.
    discard: Callable[[], None] = lambda: None


@dataclass(frozen=True)
class Finished:
    """A job that succeeded, with its inputs' and outputs' hashes by slot, or None under `Compare.STAT`."""

    made: Made
    input_hashes: dict[str, str] | None
    output_hashes: dict[str, str] | None


Ran = Union[Finished, Failed]


@dataclass(frozen=True)
class Task:
    """One job for an executor to run: the plan's words for why, and whether it reruns over an edited output."""

    job: Job
    reason: str | None = None
    keep_edited: bool = False


class Executor(Protocol):
    """What the scheduler needs of whatever starts its jobs."""

    def over(self, job: Job) -> str | None:
        """What a job needs beyond everything the executor has, as `16 cores where the run has 8`, or None."""
        ...

    @property
    def width(self) -> int:
        """How many cores' worth of jobs it runs at once, for the run's estimate of how long is left."""
        ...

    async def run(self, dag: Dag, task: Task, log: Path, options: Options, hooks: Hooks) -> Ran:
        """Run one job's checks and commands, and hash its files once it succeeds."""
        ...

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        """Run a current job's checks alone, on the files it already has."""
        ...


def hash_both(dag: Dag, job: Job) -> tuple[dict[str, str], dict[str, str]]:
    """A job's inputs' and outputs' hashes, each by slot."""
    return job.keyed(hashes(dag, job.input_paths)), job.keyed(hashes(dag, job.output_paths))


class Local:
    """Runs jobs as processes on this machine, as many at once as `queue`'s cores and memory hold."""

    def __init__(self, queue: ReadyQueue) -> None:
        self.queue = queue

    def over(self, job: Job) -> str | None:
        return too_big(job, self.queue.cores, self.queue.memory)

    @property
    def width(self) -> int:
        return self.queue.cores

    async def run(self, dag: Dag, task: Task, log: Path, options: Options, hooks: Hooks) -> Ran:
        job = task.job
        async with self.queue.slot(job):
            hooks.admitted()
            ran = await run_job(dag, job, log, options, on_start=hooks.started, on_discard=hooks.discard,
                                reason=task.reason, keep_edited=task.keep_edited)
            if isinstance(ran, Failed):
                return ran
            inline = sum(value.size for value in (*ran.inputs.values(), *ran.outputs.values())) < INLINE_HASH_BYTES
            if options.compare is not Compare.STAT and not inline:
                # Said while the job still holds its slot, so that a run on one core reports the
                # command's end before the next job starts.
                hooks.hashing()
        # With the job's cores and memory freed. Big files are hashed off the event loop, so they
        # hold up neither another job's start nor the loop; small ones where they are,
        # since a thread hop costs more than hashing a small file.
        if options.compare is Compare.STAT:
            return Finished(ran, None, None)
        if inline:
            return Finished(ran, *hash_both(dag, job))
        return Finished(ran, *await asyncio.to_thread(hash_both, dag, job))

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        async with self.queue.slot(job):
            hooks.admitted()
            hooks.started()
            return await check_job(dag, job, log, options)


class Mixed:
    """Sends the jobs of the operations in `here` to `local`, and every other job to `remote`."""

    def __init__(self, local: Local, remote: Executor, here: frozenset[str]) -> None:
        self.local = local
        self.remote = remote
        self.here = here

    def _for(self, job: Job) -> Executor:
        return self.local if job.operation in self.here else self.remote

    def over(self, job: Job) -> str | None:
        return self._for(job).over(job)

    @property
    def width(self) -> int:
        return self.local.width + self.remote.width

    async def run(self, dag: Dag, task: Task, log: Path, options: Options, hooks: Hooks) -> Ran:
        return await self._for(task.job).run(dag, task, log, options, hooks)

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        return await self._for(job).check(dag, job, log, options, hooks)
