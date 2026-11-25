from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

from .. import screening
from ..model import Dag, Job
from ..plan import Decision, Status
from ..progress import Progress
from ..state.store import Store
from .events import Event
from .executor import Executor
from .failure import DependencyFailed, Failure
from .options import Options


@dataclass(frozen=True)
class Result:
    job: Job
    failure: Failure | None = None
    log: Path | None = None
    settled: Status | None = None
    varied: bool = False

    @property
    def caused_by(self) -> int | None:
        return self.failure.first if isinstance(self.failure, DependencyFailed) else None


@dataclass(frozen=True)
class Scheduling:
    dag: Dag
    state: Store
    options: Options
    on_event: Callable[[Event], None]
    reason: Callable[[Decision], str] | None
    force: bool | frozenset[int]
    screens: screening.ScreenRun | None
    blocked: dict[int, int]
    progress: Progress
    executor: Executor
    tasks: dict[int, asyncio.Task[Result]] = field(default_factory=dict)


def forced(force: bool | frozenset[int], job: Job) -> bool:
    return force is True or (not isinstance(force, bool) and job.id in force)


async def dependency_failure(scheduling: Scheduling, dependency: int) -> int | None:
    if dependency in scheduling.blocked:
        return scheduling.blocked[dependency]
    task = scheduling.tasks.get(dependency)
    if task is not None and (result := await task).failure is not None:
        return result.caused_by or dependency
    return None


async def failed_dependency(scheduling: Scheduling, job: Job) -> int | None:
    for dependency in job.depends_on:
        if (cause := await dependency_failure(scheduling, dependency)) is not None:
            return cause
    return None
