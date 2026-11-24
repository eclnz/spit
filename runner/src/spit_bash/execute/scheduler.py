from __future__ import annotations

import asyncio
import signal
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable

from ..files import stamps, stored_each
from ..cause import Why
from ..model import Dag, Job
from ..gates import Admission
from .. import screening
from ..plan import Decision, Status, judge, without_command
from ..progress import Progress
from ..reuse import records
from ..schedule import ReadyQueue, ranks
from ..state.store import Store
from . import events
from .events import Event
from .executor import Executor, Hooks, Local, Task
from .failure import Blocked, DependencyFailed, Failure
from ..runs import keep
from .job import Failed, place_job
from .options import Options

MakeExecutor = Callable[[dict[int, float]], Executor]


def local(options: Options) -> MakeExecutor:
    return lambda ranked: Local(ReadyQueue(options.cores, options.memory, ranked))


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


async def execute(dag: Dag, decisions: tuple[Decision, ...], state: Store, options: Options,
                  on_event: Callable[[Event], None] = lambda event: None,
                  reason: Callable[[Decision], str] | None = None,
                  make_executor: MakeExecutor | None = None,
                  *, force: bool | frozenset[int] = False) -> tuple[Result, ...]:
    tasks: dict[int, asyncio.Task[Result]] = {}
    blocked = {decision.job.id: decision.caused_by or decision.job.id
               for decision in decisions if decision.status is Status.BLOCKED}
    dag.outputs.mkdir(parents=True, exist_ok=True)
    run = options.run
    compare = options.compare
    screens = (screening.ScreenRun.start(dag.screens, state.path.with_name("screens.json"), options.cores)
               if dag.screens is not None else None)


    async def dependency_failure(dependency: int) -> int | None:
        if dependency in blocked:
            return blocked[dependency]
        if dependency in tasks and (result := await tasks[dependency]).failure is not None:
            return result.caused_by or dependency
        return None

    async def failed_dependency(job: Job) -> int | None:
        for dependency in job.depends_on:
            if (cause := await dependency_failure(dependency)) is not None:
                return cause
        return None

    async def run_one(decision: Decision) -> Result:
        result = await outcome(decision)
        if screens is not None and result.failure is None and result.settled not in (Status.HELD, Status.REJECTED):
            for screen in screens.plan.after.get(result.job.id, ()):
                log = run.job_log(result.job.work.key)
                failure = await screening.evaluate(screens, dag, screen, log)
                if failure is not None:
                    result = replace(result, failure=failure, log=log)
                    break
        if result.failure is not None:
            on_event(events.NotRun(result) if result.caused_by is not None else events.JobFailed(result))
        return result

    async def outcome(decision: Decision) -> Result:
        job = decision.job
        if screens is not None and job.id in screens.plan.affected:
            return await admit(job)
        failed = await failed_dependency(job)
        if failed is not None:
            if decision.status in (Status.RUN, Status.MAYBE):
                progress.dropped(job)
            return Result(job, DependencyFailed(failed))
        if decision.status is Status.SKIP:
            return Result(job)
        if decision.status is Status.MAYBE:
            return await settle(job)
        if decision.status is Status.PLACE:
            return await place(decision)
        return await perform(decision)

    async def admit(job: Job) -> Result:
        assert screens is not None
        failures = {dependency: cause for dependency in job.depends_on
                    if (cause := await dependency_failure(dependency)) is not None}
        producers = screens.plan.producers
        before = screens.plan.before.get(job.id)
        candidates = screening.selected(screens, job.id, include_held=bool(before))
        if before and not isinstance(candidates, Admission):
            inputs = {(port, artifact.path) for port, items in candidates.inputs.items() for artifact in items}
            for screen in before:
                if (screen.port, screen.artifact.path) not in inputs:
                    continue
                if any(path is not None and producers.paths.get(path) in failures
                       for path in producers.paths_of(screen.artifact)):
                    continue
                checks = tuple(check for check in job.before if check.port == screen.port and check.path == screen.artifact.path)
                log = run.job_log(job.work.key)
                if checks:
                    log.parent.mkdir(parents=True, exist_ok=True)
                    failure = await executor.check(dag, replace(job, checks=checks), log, options, Hooks())
                    if failure is not None:
                        progress.dropped(job)
                        return Result(job, failure, log)
                failure = await screening.evaluate(screens, dag, screen, log)
                if failure is not None:
                    progress.dropped(job)
                    return Result(job, failure, log)
        selected = screening.selected(screens, job.id) if before else candidates
        if isinstance(selected, Admission):
            screens.excluded[job.id] = selected
            progress.dropped(job)
            status = Status.HELD if selected is Admission.HELD else Status.REJECTED
            return Result(job, settled=status)
        actual = screens.plan.materialize(selected)
        for dependency in actual.depends_on:
            if dependency in failures:
                progress.dropped(job)
                return Result(actual, DependencyFailed(failures[dependency]))
        forced = force is True or (not isinstance(force, bool) and job.id in force)
        return await settle(actual, forced)

    async def place(decision: Decision) -> Result:
        job, placement = decision.job, decision.placement
        assert placement is not None, "a job planned `place` has its moves"
        failure = await asyncio.to_thread(place_job, dag, placement)
        if failure is not None:
            return Result(job, failure)
        state.apply(decision.writes)
        on_event(events.Placed(job, placement.moves))
        if decision.needs_checks:
            return await perform(decision)
        return Result(job, None)

    async def settle(job: Job, forced: bool = False) -> Result:
        settled = (await asyncio.to_thread(judge, dag, job, state, forced, compare, None)
                   if job.commands else await asyncio.to_thread(without_command, dag,
                       replace(job, depends_on=()), {}, {}))
        if settled.status is not Status.RUN:
            progress.dropped(job)
        if settled.status is Status.BLOCKED:
            return Result(job, Blocked(settled))
        if settled.status is Status.SKIP:
            state.apply(settled.writes)
            on_event(events.SkippedUnchanged(job))
            return Result(job, None, None, Status.SKIP)
        result = await perform(settled)
        return replace(result, settled=settled.status)

    async def perform(decision: Decision) -> Result:
        job = decision.job
        log = run.job_log(job.work.key)
        log.parent.mkdir(parents=True, exist_ok=True)
        if decision.needs_checks:
            return await check_one(job, log)
        earlier = state.jobs.get(job.work.key)
        hooks = Hooks(admitted=lambda: progress.started(job), started=lambda: on_event(events.Started(job)),
                      hashing=lambda: on_event(events.Hashing(job)), discard=lambda: state.forget(job.work.key),
                      queued=lambda task: on_event(events.Queued(job, task)),
                      retrying=lambda attempt, why, resources: on_event(events.Retrying(job, attempt, why, resources)))
        task = Task(job, None if reason is None else reason(decision), decision.cause.why is Why.OUTPUT_CHANGED)
        ran = await executor.run(dag, task, log, options, hooks)
        if isinstance(ran, Failed):
            progress.finished(job, None)
            return Result(job, ran.failure, ran.log)
        made, input_hashes, output_hashes = ran.made, ran.input_hashes, ran.output_hashes
        state.record(job.work.key, records.after_run(job, made.inputs, made.outputs, input_hashes, output_hashes,
                                                     made.seconds, run.name, ran.attempt, ran.memory, ran.time_limit))
        if made.replaced:
            keep(run, job.work.key, earlier, ((path, at) for path, at, _ in made.replaced))
        progress.finished(job, made.seconds)
        on_event(events.Done(job, made.seconds, progress.report()))
        varied = (earlier is not None and earlier.fingerprint == job.work.fingerprint
                  and earlier.input_hashes is not None and earlier.output_hashes is not None
                  and input_hashes == earlier.input_hashes and output_hashes is not None
                  and output_hashes != earlier.output_hashes)
        return Result(job, None, log, varied=varied)

    async def check_one(job: Job, log: Path) -> Result:
        key = job.work.key
        record = state.jobs.get(key)
        failure = await executor.check(dag, job, log, options, Hooks(admitted=lambda: on_event(events.Checking(job))))
        if failure is not None:
            state.forget(key)
            return Result(job, failure, log)
        if record is not None:
            outputs = stored_each(job.keyed(stamps(dag, job.output_paths)))
            inputs = stored_each(job.keyed(stamps(dag, job.input_paths)))
            if outputs == record.outputs and inputs == record.inputs:
                state.record(key, records.checked(record, job))
        on_event(events.Checked(job))
        return Result(job, None, log)

    to_run = [decision.job for decision in decisions if decision.status in (Status.RUN, Status.MAYBE)]
    previous = {}
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is not None and record.seconds is not None:
            previous[job.id] = record.seconds
    slotted = [decision.job for decision in decisions
               if decision.status in (Status.RUN, Status.MAYBE) or decision.needs_checks]
    progress = Progress.of(to_run, dag.jobs, previous, options.cores)
    executor = (make_executor or local(options))(ranks(slotted, progress.expected, previous))
    if executor.width != options.cores:
        progress = Progress.of(to_run, dag.jobs, previous, executor.width)
    for job in slotted:
        over = executor.over(job)
        if over is not None:
            on_event(events.RunsAlone(job, over))
    places = sum(1 for decision in decisions if decision.status is Status.PLACE)
    if to_run or places:
        maybe = sum(1 for decision in decisions if decision.status is Status.MAYBE)
        on_event(events.Running(len(to_run) - maybe, maybe, progress.remaining(), places))

    loop = asyncio.get_running_loop()
    main = asyncio.current_task()
    assert main is not None
    loop.add_signal_handler(signal.SIGTERM, main.cancel)
    try:
        for decision in decisions:
            if decision.status in (Status.RUN, Status.CHECK, Status.MAYBE, Status.PLACE) or (
                    decision.status is Status.SKIP and screens is not None and decision.job.id in screens.plan.after):
                tasks[decision.job.id] = asyncio.create_task(run_one(decision))
        results = tuple(await asyncio.gather(*tasks.values()))
        if screens is not None:
            screening.publish(screens, dag, set(blocked) | {result.job.id for result in results if result.failure is not None})
            for line in screening.lines(screens):
                on_event(events.ScreenNote(line))
        return results
    finally:
        loop.remove_signal_handler(signal.SIGTERM)
        executor.stop()
