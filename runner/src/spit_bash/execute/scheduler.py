from __future__ import annotations

import asyncio
import signal
from collections import Counter
from dataclasses import replace
from pathlib import Path
from typing import Callable

from ..files import stamps, stored_each
from ..cause import Why
from ..model import Dag, Job
from .. import screening
from ..plan import Decision, Status, judge, without_command
from ..progress import Progress
from ..reuse import records
from ..schedule import ReadyQueue, ranks
from ..state.record import Record
from ..state.store import Store
from . import events
from .admit import admit
from .events import Event
from .executor import Executor, Finished, Hooks, Local, Task
from .failure import Blocked, DependencyFailed
from ..runs import keep
from .job import Failed, place_job
from .options import Options
from .scheduling import Result, Scheduling, failed_dependency, forced

__all__ = ["MakeExecutor", "Result", "execute", "local"]

MakeExecutor = Callable[[dict[int, float]], Executor]

SLOTTED = (Status.RUN, Status.MAYBE)
TASKED = (Status.RUN, Status.CHECK, Status.MAYBE, Status.PLACE)


def local(options: Options) -> MakeExecutor:
    return lambda ranked: Local(ReadyQueue(options.cores, options.memory, ranked))


def _previous_seconds(dag: Dag, state: Store) -> dict[int, float]:
    previous = {}
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is not None and record.seconds is not None:
            previous[job.id] = record.seconds
    return previous


def _varied(earlier: Record | None, job: Job, ran: Finished) -> bool:
    return (
        earlier is not None
        and earlier.fingerprint == job.work.fingerprint
        and earlier.input_hashes is not None
        and earlier.output_hashes is not None
        and ran.input_hashes == earlier.input_hashes
        and ran.output_hashes is not None
        and ran.output_hashes != earlier.output_hashes
    )


def _tasked(decision: Decision, screens: screening.ScreenRun | None) -> bool:
    if decision.status in TASKED:
        return True
    return decision.status is Status.SKIP and screens is not None and decision.job.id in screens.plan.after


async def _check_one(scheduling: Scheduling, job: Job, log: Path) -> Result:
    key = job.work.key
    record = scheduling.state.jobs.get(key)
    hooks = Hooks(admitted=lambda: scheduling.on_event(events.Checking(job)))
    failure = await scheduling.executor.check(scheduling.dag, job, log, scheduling.options, hooks)
    if failure is not None:
        scheduling.state.forget(key)
        return Result(job, failure, log)
    if record is not None:
        outputs = stored_each(job.keyed(stamps(scheduling.dag, job.output_paths)))
        inputs = stored_each(job.keyed(stamps(scheduling.dag, job.input_paths)))
        if outputs == record.outputs and inputs == record.inputs:
            scheduling.state.record(key, records.checked(record, job))
    scheduling.on_event(events.Checked(job))
    return Result(job, None, log)


def _hooks(scheduling: Scheduling, job: Job) -> Hooks:
    on_event, progress, state = scheduling.on_event, scheduling.progress, scheduling.state
    return Hooks(
        admitted=lambda: progress.started(job),
        started=lambda: on_event(events.Started(job)),
        hashing=lambda: on_event(events.Hashing(job)),
        discard=lambda: state.forget(job.work.key),
        queued=lambda task: on_event(events.Queued(job, task)),
        retrying=lambda attempt, why, resources: on_event(events.Retrying(job, attempt, why, resources)),
    )


async def _perform(scheduling: Scheduling, decision: Decision) -> Result:
    job = decision.job
    run = scheduling.options.run
    log = run.job_log(job.work.key)
    log.parent.mkdir(parents=True, exist_ok=True)
    if decision.needs_checks:
        return await _check_one(scheduling, job, log)
    earlier = scheduling.state.jobs.get(job.work.key)
    reason = None if scheduling.reason is None else scheduling.reason(decision)
    task = Task(job, reason, decision.cause.why is Why.OUTPUT_CHANGED)
    ran = await scheduling.executor.run(scheduling.dag, task, log, scheduling.options, _hooks(scheduling, job))
    if isinstance(ran, Failed):
        scheduling.progress.finished(job, None)
        return Result(job, ran.failure, ran.log)
    made = ran.made
    scheduling.state.record(
        job.work.key,
        records.after_run(
            job,
            made.inputs,
            made.outputs,
            ran.input_hashes,
            ran.output_hashes,
            made.seconds,
            run.name,
            ran.attempt,
            ran.memory,
            ran.time_limit,
        ),
    )
    if made.replaced:
        keep(run, job.work.key, earlier, ((path, at) for path, at, _ in made.replaced))
    scheduling.progress.finished(job, made.seconds)
    scheduling.on_event(events.Done(job, made.seconds, scheduling.progress.report()))
    return Result(job, None, log, varied=_varied(earlier, job, ran))


async def _place(scheduling: Scheduling, decision: Decision) -> Result:
    job, placement = decision.job, decision.placement
    assert placement is not None, "a job planned `place` has its moves"
    failure = await asyncio.to_thread(place_job, scheduling.dag, placement)
    if failure is not None:
        return Result(job, failure)
    scheduling.state.apply(decision.writes)
    scheduling.on_event(events.Placed(job, placement.moves))
    if decision.needs_checks:
        return await _perform(scheduling, decision)
    return Result(job, None)


async def _settle(scheduling: Scheduling, job: Job, force: bool = False) -> Result:
    dag, state = scheduling.dag, scheduling.state
    settled = (
        await asyncio.to_thread(judge, dag, job, state, force, scheduling.options.compare, None)
        if job.commands
        else await asyncio.to_thread(without_command, dag, replace(job, depends_on=()), {}, {})
    )
    if settled.status is not Status.RUN:
        scheduling.progress.dropped(job)
    if settled.status is Status.BLOCKED:
        return Result(job, Blocked(settled))
    if settled.status is Status.SKIP:
        state.apply(settled.writes)
        scheduling.on_event(events.SkippedUnchanged(job))
        return Result(job, None, None, Status.SKIP)
    result = await _perform(scheduling, settled)
    return replace(result, settled=settled.status)


async def _admitted(scheduling: Scheduling, job: Job) -> Result:
    admitted = await admit(scheduling, job)
    if isinstance(admitted, Result):
        scheduling.progress.dropped(job)
        return admitted
    return await _settle(scheduling, admitted, forced(scheduling.force, job))


async def _outcome(scheduling: Scheduling, decision: Decision) -> Result:
    job = decision.job
    if scheduling.screens is not None and job.id in scheduling.screens.plan.affected:
        return await _admitted(scheduling, job)
    failed = await failed_dependency(scheduling, job)
    if failed is not None:
        if decision.status in SLOTTED:
            scheduling.progress.dropped(job)
        return Result(job, DependencyFailed(failed))
    match decision.status:
        case Status.SKIP:
            return Result(job)
        case Status.MAYBE:
            return await _settle(scheduling, job)
        case Status.PLACE:
            return await _place(scheduling, decision)
        case _:
            return await _perform(scheduling, decision)


async def _screen_after(scheduling: Scheduling, result: Result) -> Result:
    screens = scheduling.screens
    if screens is None or result.failure is not None or result.settled in (Status.HELD, Status.REJECTED):
        return result
    for screen in screens.plan.after.get(result.job.id, ()):
        log = scheduling.options.run.job_log(result.job.work.key)
        failure = await screening.evaluate(screens, scheduling.dag, screen, log)
        if failure is not None:
            return replace(result, failure=failure, log=log)
    return result


async def _run_one(scheduling: Scheduling, decision: Decision) -> Result:
    result = await _screen_after(scheduling, await _outcome(scheduling, decision))
    if result.failure is not None:
        failed = events.NotRun(result) if result.caused_by is not None else events.JobFailed(result)
        scheduling.on_event(failed)
    return result


def _executor_and_progress(
    decisions: tuple[Decision, ...],
    dag: Dag,
    state: Store,
    options: Options,
    make_executor: MakeExecutor | None,
) -> tuple[Executor, Progress, list[Job]]:
    to_run = [decision.job for decision in decisions if decision.status in SLOTTED]
    slotted = [decision.job for decision in decisions if decision.status in SLOTTED or decision.needs_checks]
    previous = _previous_seconds(dag, state)
    progress = Progress.of(to_run, dag.jobs, previous, options.cores)
    executor = (make_executor or local(options))(ranks(slotted, progress.expected, previous))
    if executor.width != options.cores:
        progress = Progress.of(to_run, dag.jobs, previous, executor.width)
    return executor, progress, slotted


def _announce(
    decisions: tuple[Decision, ...],
    executor: Executor,
    progress: Progress,
    slotted: list[Job],
    on_event: Callable[[Event], None],
) -> None:
    for job in slotted:
        over = executor.over(job)
        if over is not None:
            on_event(events.RunsAlone(job, over))
    count = Counter(decision.status for decision in decisions)
    if count[Status.RUN] or count[Status.MAYBE] or count[Status.PLACE]:
        on_event(events.Running(count[Status.RUN], count[Status.MAYBE], progress.remaining(), count[Status.PLACE]))


async def execute(
    dag: Dag,
    decisions: tuple[Decision, ...],
    state: Store,
    options: Options,
    on_event: Callable[[Event], None] = lambda event: None,
    reason: Callable[[Decision], str] | None = None,
    make_executor: MakeExecutor | None = None,
    *,
    force: bool | frozenset[int] = False,
) -> tuple[Result, ...]:
    dag.outputs.mkdir(parents=True, exist_ok=True)
    screens = (
        screening.ScreenRun.start(dag.screens, state.path.with_name("screens.json"), options.cores)
        if dag.screens is not None
        else None
    )
    executor, progress, slotted = _executor_and_progress(decisions, dag, state, options, make_executor)
    _announce(decisions, executor, progress, slotted, on_event)
    scheduling = Scheduling(
        dag=dag,
        state=state,
        options=options,
        on_event=on_event,
        reason=reason,
        force=force,
        screens=screens,
        blocked={
            decision.job.id: decision.caused_by or decision.job.id
            for decision in decisions
            if decision.status is Status.BLOCKED
        },
        progress=progress,
        executor=executor,
    )

    loop = asyncio.get_running_loop()
    main = asyncio.current_task()
    assert main is not None, "execute runs inside a task"
    loop.add_signal_handler(signal.SIGTERM, main.cancel)
    try:
        for decision in decisions:
            if _tasked(decision, screens):
                scheduling.tasks[decision.job.id] = asyncio.create_task(_run_one(scheduling, decision))
        results = tuple(await asyncio.gather(*scheduling.tasks.values()))
        if screens is not None:
            failed = set(scheduling.blocked) | {result.job.id for result in results if result.failure is not None}
            screening.publish(screens, dag, failed)
            for line in screening.lines(screens):
                scheduling.on_event(events.ScreenNote(line))
        return results
    finally:
        loop.remove_signal_handler(signal.SIGTERM)
        executor.stop()
