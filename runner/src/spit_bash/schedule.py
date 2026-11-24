from __future__ import annotations

import asyncio
import heapq
from collections.abc import AsyncIterator, Callable, Sequence
from contextlib import asynccontextmanager
from statistics import median

from .model import Job


def longest_paths(jobs: Sequence[Job], weight: Callable[[Job], float]) -> tuple[dict[int, float], dict[int, float]]:
    after = {job.id: 0.0 for job in jobs}
    start: dict[int, float] = {}
    for job in reversed(jobs):
        start[job.id] = weight(job) + after[job.id]
        for dependency in job.depends_on:
            if dependency in after and after[dependency] < start[job.id]:
                after[dependency] = start[job.id]
    return start, after


def ranks(jobs: Sequence[Job], expected: Callable[[Job], float | None], previous: dict[int, float]) -> dict[int, float]:
    default = median(previous.values()) if previous else 1.0

    def weight(job: Job) -> float:
        seconds = expected(job)
        return default if seconds is None else seconds

    return longest_paths(jobs, weight)[0]


def size(memory: int) -> str:
    value, unit = float(memory), ""
    for next_unit in "KMGT":
        if value < 1024:
            break
        value, unit = value / 1024, next_unit
    return f"{value:.1f}".removesuffix(".0") + unit


def too_big(job: Job, cores: int, memory: int | None) -> str | None:
    over = []
    if job.cores > cores:
        over.append(f"{job.cores} cores where the run has {cores}")
    if memory is not None and job.memory is not None and job.memory > memory:
        over.append(f"{size(job.memory)} of memory where the run has {size(memory)}")
    return ", and ".join(over) or None


def over_memory(jobs: list[Job], memory: int | None) -> list[str]:
    if memory is None:
        return []
    return [
        f"[{job.id}] {job.operation} needs {size(job.memory)} of memory where the run has {size(memory)}"
        for job in jobs
        if job.memory is not None and job.memory > memory
    ]


class ReadyQueue:
    def __init__(self, cores: int, memory: int | None, ranks: dict[int, float]) -> None:
        self.cores = cores
        self.memory = memory
        self.free_cores = cores
        self.free_memory = memory or 0
        self._ranks = ranks
        self._waiting: list[tuple[float, int, int, int, asyncio.Future[None]]] = []
        self._admitting = False

    def needs(self, job: Job) -> tuple[int, int]:
        if too_big(job, self.cores, self.memory) is not None:
            return self.cores, self.memory or 0
        return job.cores, 0 if self.memory is None or job.memory is None else job.memory

    @asynccontextmanager
    async def slot(self, job: Job) -> AsyncIterator[None]:
        cores, memory = self.needs(job)
        admitted = asyncio.get_running_loop().create_future()
        heapq.heappush(self._waiting, (-self._ranks.get(job.id, 0.0), job.id, cores, memory, admitted))
        self._admit_soon()
        try:
            await admitted
        except asyncio.CancelledError:
            if admitted.done() and not admitted.cancelled():
                self._release(cores, memory)
            raise
        try:
            yield
        finally:
            self._release(cores, memory)

    def _release(self, cores: int, memory: int) -> None:
        self.free_cores += cores
        self.free_memory += memory
        self._admit_soon()

    def _admit_soon(self) -> None:
        if not self._admitting:
            self._admitting = True
            asyncio.get_running_loop().call_soon(self._admit)

    def _admit(self) -> None:
        self._admitting = False
        while self._waiting:
            _, _, cores, memory, admitted = self._waiting[0]
            if admitted.done():
                heapq.heappop(self._waiting)
                continue
            if cores > self.free_cores or memory > self.free_memory:
                return
            heapq.heappop(self._waiting)
            self.free_cores -= cores
            self.free_memory -= memory
            admitted.set_result(None)
