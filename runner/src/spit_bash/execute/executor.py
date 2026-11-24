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

INLINE_HASH_BYTES = 1 << 20


@dataclass(frozen=True)
class Hooks:
    admitted: Callable[[], None] = lambda: None
    started: Callable[[], None] = lambda: None
    hashing: Callable[[], None] = lambda: None
    discard: Callable[[], None] = lambda: None
    queued: Callable[[str], None] = lambda task: None
    retrying: Callable[[int, str, str], None] = lambda attempt, why, resources: None


@dataclass(frozen=True)
class Finished:
    made: Made
    input_hashes: dict[str, str] | None
    output_hashes: dict[str, str] | None
    attempt: int | None = None
    memory: int | None = None
    time_limit: float | None = None


Ran = Union[Finished, Failed]


@dataclass(frozen=True)
class Task:
    job: Job
    reason: str | None = None
    keep_edited: bool = False


class Executor(Protocol):
    def over(self, job: Job) -> str | None:
        ...

    @property
    def width(self) -> int:
        ...

    async def run(self, dag: Dag, task: Task, log: Path, options: Options, hooks: Hooks) -> Ran:
        ...

    async def check(self, dag: Dag, job: Job, log: Path, options: Options, hooks: Hooks) -> Failure | None:
        ...

    def stop(self) -> None:
        ...


def hash_both(dag: Dag, job: Job) -> tuple[dict[str, str], dict[str, str]]:
    return job.keyed(hashes(dag, job.input_paths)), job.keyed(hashes(dag, job.output_paths))


class Local:
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
                hooks.hashing()
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

    def stop(self) -> None:
        pass


class Mixed:
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

    def stop(self) -> None:
        self.local.stop()
        self.remote.stop()
