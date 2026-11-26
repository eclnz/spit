from __future__ import annotations

import asyncio
import contextlib
import signal
import sys
from dataclasses import dataclass, replace
from pathlib import Path

from . import confirm, runs, tools
from .cause import Why
from .execute.options import Options
from .execute.scheduler import Result, execute
from .leftovers import Leftover, leftovers, lines as leftover_lines, remove as remove_leftovers
from .model import Dag, DagError, Layout
from .plan import Decision, Status, explained, plan, writes
from .report import run_text
from .report.causes import decided
from .report.changes import change_lines, save_sources, saved_sources
from .report.explain import explain
from .report.plan_json import document
from .report.plan_text import listing, move_hint, nothing_run
from .report.reviews import review_json, review_lines, review_reports
from .report.summary import Summary, left_out
from .reuse.evidence import Compare
from .reuse.relabel import relabel
from .review_sheets import prepare_sheets
from .schedule import over_memory
from .selection import ForceSelection, Selection
from .slurm import calllog, site
from .slurm.controller import Controller, Start, folder as slurm_folder, planned, set_up_path, settle
from .slurm.profile import load as load_profile
from .state.store import Store


@dataclass(frozen=True)
class PlanningOptions:
    compare: Compare
    forced: ForceSelection
    move_files: bool
    no_upstream: bool
    slurm: bool
    profile: Path | None


@dataclass(frozen=True)
class PlanCommand:
    planning: PlanningOptions
    verbose: bool
    json: bool


@dataclass(frozen=True)
class ExplainCommand:
    planning: PlanningOptions
    job: int


@dataclass(frozen=True)
class RunCommand:
    planning: PlanningOptions
    options: Options
    verbose: bool
    partial: bool
    bypass_verdicts: bool
    remove_leftovers: bool
    confirm_replacements: bool
    ask_over: float


PlanningCommand = PlanCommand | ExplainCommand | RunCommand


def plan_or_run(
    command: PlanningCommand,
    dag: Dag,
    everything: Dag,
    selection: Selection,
    state: Store,
    layout: Layout,
) -> int:
    settings = command.planning
    running = isinstance(command, RunCommand)
    run = command.options.run if isinstance(command, RunCommand) else None
    slurm = settings.slurm
    pending_screens = dag.screens is not None and any(
        job.id in dag.screens.before or job.id in dag.screens.after for job in dag.jobs
    )
    if pending_screens and running:
        if slurm:
            raise DagError("screen verdicts during a run require --executor local")
        if settings.move_files:
            raise DagError("--move-files requires a plan with settled gate membership")
    controller: Controller | None = None
    start: Start | None = None
    call_log_scope = contextlib.ExitStack()
    try:
        if slurm or slurm_folder(layout).exists():
            profile = load_profile(settings.profile.expanduser() if settings.profile is not None else None)
            if slurm:
                set_up_path(profile)
                if running:
                    for note in site.gate(profile, layout):
                        print(f"note: {note}", file=sys.stderr)
            controller = Controller(profile, layout)
            if running:
                controller.lease()
                call_log_scope.enter_context(calllog.capture(controller.folder / "call_logs.jsonl"))
            start = controller.start(everything, state, running)
            for line in start.lines:
                print(line, file=sys.stderr)
            if start.live and running and not slurm:
                raise DagError(
                    f"{start.live} Slurm task(s) of this folder are still queued or running. Wait for them, "
                    "run with --executor slurm to take them over, or cancel them with scancel."
                )
            settle(everything, state, profile.visibility)
        return _prepare_plan(command, dag, everything, selection, state, layout, controller, start)
    except (DagError, OSError) as exc:
        if not slurm or run is None:
            raise
        print(f"error: {exc}", file=sys.stderr)
        runs.finish(run, runs.Ending.SETUP_FAILED, {})
        return 2
    except KeyboardInterrupt:
        if not slurm or run is None:
            raise
        print("interrupted during Slurm setup or reconciliation; outstanding reservations kept", file=sys.stderr)
        runs.finish(run, runs.Ending.INTERRUPTED, {})
        return 128 + signal.SIGINT
    finally:
        try:
            call_log_scope.close()
        finally:
            if controller is not None:
                controller.close()


def _prepare_plan(
    command: PlanningCommand,
    dag: Dag,
    everything: Dag,
    selection: Selection,
    state: Store,
    layout: Layout,
    controller: Controller | None,
    start: Start | None,
) -> int:
    settings = command.planning
    compare, forced = settings.compare, settings.forced
    if compare is not Compare.STAT:
        handed = relabel(dag, state.jobs, everything)
        if isinstance(command, RunCommand):
            state.apply(handed)
        else:
            state.assume(handed)
    claimed_dag = everything if settings.no_upstream else None
    decisions = plan(dag, state, forced, compare, settings.move_files and forced is False, claimed_dag=claimed_dag)
    movable = 0
    if (
        not settings.move_files
        and forced is False
        and any(
            decision.status is Status.RUN and decision.cause.why is Why.OUTPUT_MISSING and decision.cause.paths
            for decision in decisions
        )
    ):
        moved = plan(dag, state, forced, compare, True, claimed_dag=claimed_dag)
        movable = sum(decision.status is Status.PLACE for decision in moved)
    if isinstance(command, RunCommand):
        run = command.options.run
        bypass = command.bypass_verdicts
        if bypass:
            print("review and screen verdicts bypassed", file=sys.stderr)
        state.apply(writes(decisions))
        run.plan.write_text(
            document(dag, decisions, review_json(everything, decisions), bypass_verdicts=bypass) + "\n",
            encoding="utf-8",
        )
    blocked = any(decision.status is Status.BLOCKED for decision in decisions)
    if isinstance(command, ExplainCommand):
        decision, evidence = explained(dag, state, decisions, command.job, compare)
        print("\n".join(explain(dag, decisions, decision, evidence, layout.runs)))
        return 1 if decision.status is Status.BLOCKED else 0
    if isinstance(command, PlanCommand) and command.json:
        print(document(dag, decisions, review_json(everything, decisions)))
    else:
        for line in change_lines(everything, decisions, state.jobs, saved_sources(layout.sources)):
            print(line)
        for line in listing(dag, decisions, command.verbose):
            print(line)
        if isinstance(command, PlanCommand):
            for line in review_lines(review_reports(everything, decisions)):
                print(line)
    if movable:
        print(move_hint(movable), file=sys.stderr)
    found = () if everything.left_out else leftovers(everything, state.jobs)
    if isinstance(command, PlanCommand):
        for line in leftover_lines(found, command.verbose, False):
            print(line, file=sys.stderr)
        return 1 if blocked else 0
    return _run_plan(command, dag, everything, selection, state, layout, decisions, blocked, found, controller, start)


def _run_plan(
    command: RunCommand,
    dag: Dag,
    everything: Dag,
    selection: Selection,
    state: Store,
    layout: Layout,
    decisions: tuple[Decision, ...],
    blocked: bool,
    found: tuple[Leftover, ...],
    controller: Controller | None,
    start: Start | None,
) -> int:
    options = command.options
    run = options.run
    slurm = controller is not None and command.planning.slurm
    removing = command.remove_leftovers
    if blocked and not command.partial:
        print(nothing_run(decisions), file=sys.stderr)
        runs.finish(run, runs.Ending.BLOCKED, {})
        return 1

    if not slurm and options.enforce_memory:
        running = [d.job for d in decisions if d.status in (Status.RUN, Status.MAYBE)]
        over = over_memory(running, options.memory)
        if over:
            for line in over:
                print(line, file=sys.stderr)
            print(
                "lower the job's `resources` line, give more with --memory, "
                "or pass --no-enforce-memory to run it alone",
                file=sys.stderr,
            )
            runs.finish(run, runs.Ending.OVER_MEMORY, {})
            return 2

    if command.confirm_replacements and sys.stdin.isatty():
        costly = confirm.costly(decisions, state.jobs, command.ask_over)
        if costly:
            for line in confirm.lines(costly):
                print(line, file=sys.stderr)
            print("Continue? [y/N] ", end="", file=sys.stderr, flush=True)
            if sys.stdin.readline().strip().lower() not in ("y", "yes"):
                print("nothing was run", file=sys.stderr)
                runs.finish(run, runs.Ending.DECLINED, {})
                return 1

    for line in leftover_lines(found, command.verbose, removing):
        print(line, file=sys.stderr)
    if removing and found:
        state.apply(remove_leftovers(everything, found))

    save_sources(layout.sources, everything, saved_sources(layout.sources) or () if everything.left_out else ())
    make_executor = None
    if slurm:
        assert controller is not None, "a Slurm run has a controller"
        options = replace(options, cores=controller.profile.local_cores, memory=None, enforce_memory=False)
        assert start is not None, "a Slurm run read the queue as it started"
        wanted = planned(decisions)
        cancelled = controller.act(wanted, start)
        if cancelled:
            print(f"cancelled {cancelled} Slurm task(s) of work this run does not do", file=sys.stderr)
        make_executor = controller.executor(dag, options, state, wanted, start)
    try:
        results = asyncio.run(
            execute(
                dag,
                decisions,
                state,
                options,
                run_text.printer(dag),
                decided,
                make_executor,
                force=command.planning.forced,
            )
        )
    except asyncio.CancelledError:
        print(
            "terminated; see the Slurm cancellation report" if slurm else "terminated; running jobs were stopped",
            file=sys.stderr,
        )
        runs.finish(run, runs.Ending.TERMINATED, {})
        return 128 + signal.SIGTERM
    except KeyboardInterrupt:
        print(
            "interrupted; see the Slurm cancellation report" if slurm else "interrupted; running jobs were stopped",
            file=sys.stderr,
        )
        runs.finish(run, runs.Ending.INTERRUPTED, {})
        return 128 + signal.SIGINT
    failed = _finish_run(command, dag, everything, selection, state, layout, decisions, results, controller)
    return 1 if failed or blocked else 0


def _finish_run(
    command: RunCommand,
    dag: Dag,
    everything: Dag,
    selection: Selection,
    state: Store,
    layout: Layout,
    decisions: tuple[Decision, ...],
    results: tuple[Result, ...],
    controller: Controller | None,
) -> bool:
    run = command.options.run
    summary = Summary.of(decisions, results)
    run_text.report_failures(dag, summary, command.verbose)
    if summary.placed:
        print(run_text.moved_note(), file=sys.stderr)
    for note in run_text.varied_notes(results):
        print(note, file=sys.stderr)
    for line in run_text.replaced_lines(run):
        print(line, file=sys.stderr)
    gates = {
        status.value: sum(result.settled is status for result in results)
        + sum(job.status.name.lower() == status.value for job in everything.gated)
        for status in (Status.HELD, Status.REJECTED)
    }
    print(run_text.summary_line(summary, gates[Status.HELD.value], gates[Status.REJECTED.value]), file=sys.stderr)
    reviews = review_reports(everything, decisions, results)
    failed = bool(summary.failed or summary.not_run)
    try:
        created = prepare_sheets(reviews, decisions)
    except DagError as exc:
        print(f"error: {exc}", file=sys.stderr)
        failed = True
        created = set()
    for line in review_lines(reviews, after_run=True, created=created):
        print(line, file=sys.stderr)
    if command.planning.slurm and controller is not None:
        for line in controller.short_lines(results, state.jobs, run.name):
            print(line, file=sys.stderr)
    if command.verbose and everything.gated:
        for job in everything.gated:
            for path in job.outputs:
                if everything.path(path).exists():
                    print(f"  {path} remains from an earlier run ({job.status.name.lower()})", file=sys.stderr)
    if selection:
        for line in run_text.left_lines(left_out(everything, dag, decisions, results, state, command.planning.compare)):
            print(line, file=sys.stderr)
    if dag.tools:
        tools.save(layout.tools, dag.tools)
    counts = run_text.counts(summary)
    counts.update((status, count) for status, count in gates.items() if count)
    if command.bypass_verdicts:
        counts["bypass_verdicts"] = True
    runs.finish(run, runs.Ending.FINISHED, counts)
    return failed
