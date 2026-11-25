from __future__ import annotations

from dataclasses import replace

from .. import screening
from ..gates import Admission
from ..model import Job
from ..plan import Status
from ..schema import SpitJob
from .executor import Hooks
from .failure import DependencyFailed
from .scheduling import Result, Scheduling, dependency_failure


def _producer_failed(screens: screening.ScreenRun, screen: screening.BoundScreen, failures: dict[int, int]) -> bool:
    producers = screens.plan.producers
    return any(
        path is not None and producers.paths.get(path) in failures for path in producers.paths_of(screen.artifact)
    )


def _screens_to_run(
    screens: screening.ScreenRun,
    before: list[screening.BoundScreen],
    candidates: SpitJob,
    failures: dict[int, int],
) -> list[screening.BoundScreen]:
    inputs = {(port, artifact.path) for port, items in candidates.inputs.items() for artifact in items}
    return [
        screen
        for screen in before
        if (screen.port, screen.artifact.path) in inputs and not _producer_failed(screens, screen, failures)
    ]


async def _screen_inputs(
    scheduling: Scheduling,
    screens: screening.ScreenRun,
    job: Job,
    before: list[screening.BoundScreen],
) -> Result | None:
    log = scheduling.options.run.job_log(job.work.key)
    for screen in before:
        checks = tuple(
            check for check in job.before if check.port == screen.port and check.path == screen.artifact.path
        )
        if checks:
            log.parent.mkdir(parents=True, exist_ok=True)
            narrowed = replace(job, checks=checks)
            failure = await scheduling.executor.check(scheduling.dag, narrowed, log, scheduling.options, Hooks())
            if failure is not None:
                return Result(job, failure, log)
        failure = await screening.evaluate(screens, scheduling.dag, screen, log)
        if failure is not None:
            return Result(job, failure, log)
    return None


async def admit(scheduling: Scheduling, job: Job) -> Result | Job:
    screens = scheduling.screens
    assert screens is not None, "only a job a screen affects is admitted"
    failures = {
        dependency: cause
        for dependency in job.depends_on
        if (cause := await dependency_failure(scheduling, dependency)) is not None
    }
    before = screens.plan.before.get(job.id)
    candidates = screening.selected(screens, job.id, include_held=bool(before))
    if before and not isinstance(candidates, Admission):
        screened = await _screen_inputs(
            scheduling, screens, job, _screens_to_run(screens, before, candidates, failures)
        )
        if screened is not None:
            return screened
    selected = screening.selected(screens, job.id) if before else candidates
    if isinstance(selected, Admission):
        screens.excluded[job.id] = selected
        return Result(job, settled=Status.HELD if selected is Admission.HELD else Status.REJECTED)
    actual = screens.plan.materialize(selected)
    for dependency in actual.depends_on:
        if dependency in failures:
            return Result(actual, DependencyFailed(failures[dependency]))
    return actual
