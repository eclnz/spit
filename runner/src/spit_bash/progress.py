from __future__ import annotations

import heapq
import time
from dataclasses import dataclass, field

from .model import Job
from .schedule import longest_paths

REFRESH_SECONDS = 5.0


def duration(seconds: float) -> str:
    if seconds < 1:
        return "under 1s"
    seconds = round(seconds)
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{round(seconds / 60)}m"
    hours, minutes = divmod(round(seconds / 60), 60)
    return f"{hours}h{minutes:02d}m"


def about(seconds: float) -> str:
    return "under 1s" if seconds < 1 else f"about {duration(seconds)}"


@dataclass
class _Operation:
    total: float = 0.0
    count: int = 0
    known: float = 0.0
    unknown: int = 0

    def mean(self) -> float | None:
        return self.total / self.count if self.count else None


@dataclass
class Progress:
    cores: int
    left: int = 0
    _width: int = 0
    _operations: dict[str, _Operation] = field(default_factory=dict)
    _own: dict[int, float] = field(default_factory=dict)
    _started: dict[int, tuple[Job, float]] = field(default_factory=dict)
    _jobs: list[Job] = field(default_factory=list)
    _waiting: set[int] = field(default_factory=set)
    _after: dict[int, float] = field(default_factory=dict)
    _longest: list[tuple[float, int]] = field(default_factory=list)
    _paths_at: float | None = None

    @classmethod
    def of(cls, to_run: list[Job], dag_jobs: tuple[Job, ...], previous: dict[int, float], cores: int) -> Progress:
        progress = cls(cores, len(to_run), _jobs=list(to_run), _waiting={job.id for job in to_run})
        for job in dag_jobs:
            if job.id in previous:
                operation = progress._operation(job)
                operation.total += previous[job.id]
                operation.count += 1
        for job in to_run:
            operation = progress._operation(job)
            progress._width += progress._cores(job)
            if job.id in previous:
                progress._own[job.id] = previous[job.id]
                operation.known += previous[job.id] * progress._cores(job)
            else:
                operation.unknown += progress._cores(job)
        return progress

    def _operation(self, job: Job) -> _Operation:
        return self._operations.setdefault(job.operation, _Operation())

    def _cores(self, job: Job) -> int:
        return min(job.cores, self.cores)

    def expected(self, job: Job) -> float | None:
        if job.id in self._own:
            return self._own[job.id]
        operation = self._operations.get(job.operation)
        return None if operation is None else operation.mean()

    def _remove(self, job: Job) -> None:
        operation = self._operation(job)
        if job.id in self._own:
            operation.known -= self._own[job.id] * self._cores(job)
        else:
            operation.unknown -= self._cores(job)
        self.left -= 1
        self._waiting.discard(job.id)

    def started(self, job: Job) -> None:
        self._remove(job)
        self._started[job.id] = (job, time.monotonic())

    def finished(self, job: Job, seconds: float | None) -> None:
        del self._started[job.id]
        self._width -= self._cores(job)
        if seconds is not None:
            operation = self._operation(job)
            operation.total += seconds
            operation.count += 1

    def dropped(self, job: Job) -> None:
        self._remove(job)
        self._width -= self._cores(job)

    def remaining(self) -> float | None:
        work = 0.0
        for operation in self._operations.values():
            mean = operation.mean()
            if operation.unknown and mean is None:
                return None
            work += operation.known + operation.unknown * (mean or 0.0)
        now = time.monotonic()
        for job, start in self._started.values():
            expected = self.expected(job)
            if expected is None:
                return None
            work += max(expected - (now - start), 0.0) * self._cores(job)
        if self.left + len(self._started) == 0:
            return 0.0
        return max(work / min(self.cores, self._width), self._critical(now))

    def _critical(self, now: float) -> float:
        if self._paths_at is None or now - self._paths_at >= REFRESH_SECONDS:
            self._paths(now)
        while self._longest and self._longest[0][1] not in self._waiting:
            heapq.heappop(self._longest)
        longest = -self._longest[0][0] if self._longest else 0.0
        for job, start in self._started.values():
            left = max((self.expected(job) or 0.0) - (now - start), 0.0)
            longest = max(longest, left + self._after.get(job.id, 0.0))
        return longest

    def _paths(self, now: float) -> None:
        self._jobs = [job for job in self._jobs if job.id in self._waiting or job.id in self._started]
        start, self._after = longest_paths(self._jobs, lambda job: self.expected(job) or 0.0)
        self._longest = [(-start[job.id], job.id) for job in self._jobs if job.id in self._waiting]
        heapq.heapify(self._longest)
        self._paths_at = now

    def report(self) -> str:
        jobs = self.left + len(self._started)
        remaining = self.remaining() if jobs else None
        return f"{jobs} left" if remaining is None else f"{jobs} left, {about(remaining)}"
