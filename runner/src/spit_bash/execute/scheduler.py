"""Run every job a plan says to, concurrently within the run's cores and memory, and record what succeeds."""

from __future__ import annotations

import asyncio
import signal
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable

from ..files import stamps, stored_each
from ..cause import Why
from ..model import Dag, Job
from ..plan import Decision, Status, judge
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

# Makes the run's executor, given each job's rank: its longest expected path to the end of the run.
MakeExecutor = Callable[[dict[int, float]], Executor]


def local(options: Options) -> MakeExecutor:
    """Run every job on this machine, within `options`' cores and memory."""
    return lambda ranked: Local(ReadyQueue(options.cores, options.memory, ranked))


@dataclass(frozen=True)
class Result:
    job: Job
    failure: Failure | None = None
    log: Path | None = None
    # What a job planned `maybe` turned out to be, once its upstream jobs had finished.
    settled: Status | None = None
    # The job ran its recorded work on the inputs its record holds, and made outputs that differ
    # from the record's: its tool doesn't give the same bytes twice.
    varied: bool = False

    @property
    def caused_by(self) -> int | None:
        """For a job that did not run because a job it needs failed: the job whose failure it is."""
        return self.failure.first if isinstance(self.failure, DependencyFailed) else None


async def execute(dag: Dag, decisions: tuple[Decision, ...], state: Store, options: Options,
                  on_event: Callable[[Event], None] = lambda event: None,
                  reason: Callable[[Decision], str] | None = None,
                  make_executor: MakeExecutor | None = None) -> tuple[Result, ...]:
    """Run every job planned to run, check or maybe run, and move the outputs of each planned `place`;
    stop them all if the process gets SIGTERM.

    `make_executor` makes what starts the jobs; by default, `local`, on this machine.

    Each job that succeeds is recorded in `state` as it ends, and a placed job
    as soon as its files are moved. `on_event` hears, in order, what the run is
    about to do and each job's progress; the run itself prints nothing.
    `reason` gives the words for why a job runs, which start its log.
    """
    tasks: dict[int, asyncio.Task[Result]] = {}
    # Commands run from the output root, which nothing may have made yet.
    dag.outputs.mkdir(parents=True, exist_ok=True)
    run = options.run
    compare = options.compare

    async def failed_dependency(job: Job) -> int | None:
        """Wait for the jobs this one needs, and name the job whose failure stopped the first that failed."""
        for dependency in job.depends_on:
            if dependency in tasks and (result := await tasks[dependency]).failure is not None:
                return result.caused_by or dependency
        return None

    async def run_one(decision: Decision) -> Result:
        result = await outcome(decision)
        if result.failure is not None:
            on_event(events.NotRun(result) if result.caused_by is not None else events.JobFailed(result))
        return result

    async def outcome(decision: Decision) -> Result:
        job = decision.job
        failed = await failed_dependency(job)
        if failed is not None:
            if decision.status in (Status.RUN, Status.MAYBE):
                progress.dropped(job)
            return Result(job, DependencyFailed(failed))
        if decision.status is Status.MAYBE:
            return await settle(job)
        if decision.status is Status.PLACE:
            return await place(decision)
        return await perform(decision)

    async def place(decision: Decision) -> Result:
        """Move a job's recorded outputs to its paths, record them there at once, then run its checks if it needs them."""
        job, placement = decision.job, decision.placement
        assert placement is not None, "a job planned `place` has its moves"
        # Off the event loop: checking a moved folder reads every file under it.
        failure = await asyncio.to_thread(place_job, dag, placement)
        if failure is not None:
            return Result(job, failure)
        # Recorded before anything else happens, so a run stopped from here on finds the files.
        state.apply(decision.writes)
        on_event(events.Placed(job, placement.moves))
        if decision.needs_checks:
            return await perform(decision)
        return Result(job, None)

    async def settle(job: Job) -> Result:
        """Judge a job planned `maybe` again, now that its upstream jobs have finished."""
        # Off the event loop: it may hash files the upstream jobs rewrote.
        settled = await asyncio.to_thread(judge, dag, job, state, False, compare, None)
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
        # The job's record stays until it succeeds and is replaced: a failure or a cancellation
        # puts the earlier run's outputs back, and the record still describes them. A runner
        # killed mid-job leaves files at the job's paths that do not match it, so the next plan
        # runs the job again.
        hooks = Hooks(admitted=lambda: progress.started(job), started=lambda: on_event(events.Started(job)),
                      hashing=lambda: on_event(events.Hashing(job)), discard=lambda: state.forget(job.work.key))
        task = Task(job, None if reason is None else reason(decision), decision.cause.why is Why.OUTPUT_CHANGED)
        ran = await executor.run(dag, task, log, options, hooks)
        if isinstance(ran, Failed):
            progress.finished(job, None)
            return Result(job, ran.failure, ran.log)
        made, input_hashes, output_hashes = ran.made, ran.input_hashes, ran.output_hashes
        state.record(job.work.key, records.after_run(job, made.inputs, made.outputs, input_hashes, output_hashes,
                                                     made.seconds, run.name))
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
        """Run a current job's checks alone, on the files it already has."""
        key = job.work.key
        record = state.jobs.get(key)
        failure = await executor.check(dag, job, log, options, Hooks(admitted=lambda: on_event(events.Checking(job))))
        if failure is not None:
            # The files fail the checks, so the job is no longer current.
            state.forget(key)
            return Result(job, failure, log)
        if record is not None:
            # A command could change a file under a check; record the stamps it passed on.
            outputs = stored_each(job.keyed(stamps(dag, job.output_paths)))
            inputs = stored_each(job.keyed(stamps(dag, job.input_paths)))
            if outputs == record.outputs and inputs == record.inputs:
                state.record(key, records.checked(record, job))
        on_event(events.Checked(job))
        return Result(job, None, log)

    # A job planned `maybe` counts as one that runs until it is skipped, so the
    # estimate is the longest the run can take.
    to_run = [decision.job for decision in decisions if decision.status in (Status.RUN, Status.MAYBE)]
    previous = {}
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is not None and record.seconds is not None:
            previous[job.id] = record.seconds
    # Every job that will take cores and memory, ranked by the longest chain of work it heads.
    slotted = [decision.job for decision in decisions
               if decision.status in (Status.RUN, Status.MAYBE) or decision.needs_checks]
    progress = Progress.of(to_run, dag.jobs, previous, options.cores)
    executor = (make_executor or local(options))(ranks(slotted, progress.expected, previous))
    if executor.width != options.cores:
        # The estimate shares out the executor's width, which only a remote executor makes other than the run's cores.
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
            if decision.status in (Status.RUN, Status.CHECK, Status.MAYBE, Status.PLACE):
                tasks[decision.job.id] = asyncio.create_task(run_one(decision))
        return tuple(await asyncio.gather(*tasks.values()))
    finally:
        loop.remove_signal_handler(signal.SIGTERM)
