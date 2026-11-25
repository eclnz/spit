from __future__ import annotations

import os
import shutil
from dataclasses import dataclass, replace
from enum import Enum
from typing import Literal

from .cause import Cause, Tool, Why
from .files import Stamps, stamp, stamps
from .model import Dag, DagError, Job
from .reuse import records, relocate
from .reuse.evidence import Compare, Evidence, changed_parts, evidence
from .reuse.relocate import Migration, MoveCause
from .reuse.verdict import Standing, verdict
from .selection import ForceSelection, is_forced
from .state.store import StateWrite, Store


class Status(Enum):
    RUN = "run"
    CHECK = "check"
    SKIP = "skip"
    BLOCKED = "blocked"
    HELD = "held"
    REJECTED = "rejected"
    MAYBE = "maybe"
    PLACE = "place"


PlanStatus = Literal[Status.RUN, Status.CHECK, Status.SKIP, Status.BLOCKED, Status.MAYBE, Status.PLACE]


@dataclass(frozen=True)
class Decision:
    job: Job
    status: PlanStatus
    cause: Cause
    writes: tuple[StateWrite, ...] = ()
    waits_for: tuple[int, ...] = ()
    caused_by: int | None = None
    placement: Migration | None = None
    replaces: bool = False

    @property
    def needs_checks(self) -> bool:
        return self.status is Status.CHECK or (self.status is Status.PLACE and self.cause.why is not Why.CURRENT)


def _missing_programs(dag: Dag) -> set[str]:
    missing = {name for name in dag.executables if shutil.which(name) is None}
    for job in dag.jobs:
        runs = (run.arguments for run in job.commands if run.calculation is None)
        for command in (*runs, *job.verify, *(check.command for check in job.checks)):
            if "/" in command[0] and command[0] not in missing:
                program = dag.outputs / command[0]
                if not (program.is_file() and os.access(program, os.X_OK)):
                    missing.add(command[0])
    return missing


UPSTREAM = (Status.RUN, Status.MAYBE)


def _missing(dag: Dag, job: Job, state: Store) -> Cause:
    record = state.jobs.get(job.work.key)
    here = job.output_paths_by_slot
    for key, old in sorted((record.output_paths or {}).items() if record else ()):
        if key in here and old != here[key] and old not in job.output_paths:
            try:
                present = dag.checked(old).exists()
            except DagError:
                continue
            if present:
                return Cause(Why.OUTPUT_MISSING, (old,))
    return Cause(Why.OUTPUT_MISSING)


def judge(dag: Dag, job: Job, state: Store, force: bool, compare: Compare, seen: Stamps | None = None) -> Decision:
    if job.missing is not None:
        return Decision(job, Status.BLOCKED, Cause(Why.MISSING_MATCH, (job.missing,)))
    now_in, now_out = stamps(dag, job.input_paths, seen), stamps(dag, job.output_paths, seen)
    if any(value is None for value in now_out.values()):
        return Decision(job, Status.RUN, _missing(dag, job, state))
    if any(value is None for value in now_in.values()):
        return Decision(job, Status.BLOCKED, Cause(Why.INPUT_MISSING))
    if force:
        return Decision(job, Status.RUN, Cause(Why.FORCED))
    found = evidence(dag, job, state.jobs.get(job.work.key), {**now_in, **now_out}, compare)
    judged = verdict(found)
    writes = () if judged.refreshed is None else (StateWrite(job.work.key, judged.refreshed),)
    match judged.standing:
        case Standing.STALE:
            assert judged.cause is not None, "a stale verdict has a cause"
            return Decision(job, Status.RUN, judged.cause)
        case Standing.CHECKS_ONLY:
            assert found.record is not None, "a verdict of checks only has a record"
            why = Why.CHECKS_CHANGED if found.record.checks is not None else Why.CHECKS_NOT_RUN
            return Decision(job, Status.CHECK, Cause(why), writes)
        case Standing.CURRENT:
            return Decision(job, Status.SKIP, Cause(Why.CURRENT), writes)


def without_command(dag: Dag, job: Job, by_id: dict[int, Decision], seen: Stamps) -> Decision:
    if job.missing is not None:
        return Decision(job, Status.BLOCKED, Cause(Why.MISSING_MATCH, (job.missing,)))
    outputs = stamps(dag, job.output_paths, seen)
    if any(value is None for value in outputs.values()):
        return Decision(job, Status.BLOCKED, Cause(Why.NO_COMMAND_OUTPUT_MISSING))
    if any(by_id[dep].status is Status.RUN for dep in job.depends_on):
        return Decision(job, Status.BLOCKED, Cause(Why.NO_COMMAND_DEPENDENCY_WILL_RUN))
    if any(by_id[dep].status is Status.MAYBE for dep in job.depends_on):
        return Decision(job, Status.BLOCKED, Cause(Why.NO_COMMAND_DEPENDENCY_MAY_RUN))
    inputs = stamps(dag, job.input_paths, seen)
    if any(value is None for value in inputs.values()):
        return Decision(job, Status.BLOCKED, Cause(Why.INPUT_MISSING))
    if job.input_paths and max(value.mtime_ns for value in inputs.values()) > min(
        value.mtime_ns for value in outputs.values()
    ):
        return Decision(job, Status.BLOCKED, Cause(Why.NO_COMMAND_OUTPUT_OLDER))
    if job.checks:
        return Decision(job, Status.CHECK, Cause(Why.NO_COMMAND_CHECKING))
    return Decision(job, Status.SKIP, Cause(Why.NO_COMMAND_OUTPUTS_EXIST))


def _below_rerun(dag: Dag, job: Job, state: Store, force: bool, compare: Compare, seen: Stamps) -> Decision:
    if compare is Compare.STAT or any(value is None for value in stamps(dag, job.input_paths, seen).values()):
        return Decision(job, Status.RUN, Cause(Why.DEPENDENCY_WILL_RUN))
    decision = judge(dag, job, state, force, compare, seen)
    if decision.status in (Status.SKIP, Status.CHECK):
        return Decision(job, Status.MAYBE, Cause(Why.UPSTREAM_MAY_RUN))
    return decision


def _placed(
    dag: Dag, decision: Decision, state: Store, compare: Compare, seen: Stamps, claimed: set[str], moving: set[str]
) -> Decision:
    job = decision.job
    found = relocate.movable(dag, job, state.jobs.get(job.work.key), claimed, moving, seen)
    if found is None:
        return decision
    if not found.moves:
        return replace(decision, placement=found)
    arrived = {new: stamp(dag.checked(old), new in dag.folders) for old, new in found.moves}
    moved = judge(dag, job, state, False, compare, {**seen, **arrived})
    if moved.status not in (Status.SKIP, Status.CHECK):
        return replace(decision, placement=replace(found, moves=(), cause=MoveCause.INPUT_CHANGED, record=None))
    seen.update(arrived)
    moving.update(arrived)
    assert found.record is not None, "a job with moves has a record"
    writes = moved.writes or (StateWrite(job.work.key, records.relocated(found.record, job)),)
    return Decision(job, Status.PLACE, moved.cause, writes, placement=found)


def _with_programs(decision: Decision, missing_programs: set[str]) -> Decision:
    job = decision.job
    match decision.status:
        case Status.RUN | Status.MAYBE:
            commands = [
                *(check.command for check in job.checks),
                *(run.arguments for run in job.commands if run.calculation is None),
                *job.verify,
            ]
            scripts = (*job.scripts, *job.check_scripts)
        case Status.CHECK | Status.PLACE:
            commands = [check.command for check in job.checks] if decision.needs_checks else []
            scripts = job.check_scripts if decision.needs_checks else ()
        case Status.SKIP | Status.BLOCKED:
            return decision
    programs = sorted({command[0] for command in commands} & missing_programs)
    if programs:
        return Decision(job, Status.BLOCKED, Cause(Why.PROGRAM_NOT_FOUND, tuple(programs)))
    absent = sorted(path for path in scripts if not os.path.exists(path))
    if absent:
        return Decision(job, Status.BLOCKED, Cause(Why.HERE_NOT_FOUND, tuple(absent)))
    return decision


def _located(decision: Decision, by_id: dict[int, Decision]) -> Decision:
    job = decision.job
    waits = tuple(dep for dep in job.depends_on if by_id[dep].status in UPSTREAM)
    cause = None
    if decision.cause.why is Why.DEPENDENCY_BLOCKED:
        first = decision.cause.jobs[0]
        cause = by_id[first].caused_by or first
    return replace(decision, waits_for=waits if decision.status in UPSTREAM else (), caused_by=cause)


def plan(
    dag: Dag,
    state: Store,
    force: ForceSelection = False,
    compare: Compare = Compare.CONTENT,
    move_files: bool = False,
    *,
    claimed_dag: Dag | None = None,
) -> tuple[Decision, ...]:
    decisions: list[Decision] = []
    by_id: dict[int, Decision] = {}
    seen: Stamps = {}
    claimed = relocate.claimed(claimed_dag or dag) if move_files else set()
    moving: set[str] = set()
    missing_external = {path for path, value in stamps(dag, dag.external_inputs, seen).items() if value is None}
    missing_programs = _missing_programs(dag)

    def decide(job: Job) -> Decision:
        if dag.screens is not None and job.id in dag.screens.affected:
            return Decision(job, Status.MAYBE, Cause(Why.PENDING_SCREEN))
        if job.missing is not None:
            return Decision(job, Status.BLOCKED, Cause(Why.MISSING_MATCH, (job.missing,)))
        forced = is_forced(force, job.id)
        missing = [path for path in job.input_paths if path in missing_external]
        if missing:
            return Decision(job, Status.BLOCKED, Cause(Why.MISSING_EXTERNAL, tuple(missing)))
        blocked = [dep for dep in job.depends_on if by_id[dep].status is Status.BLOCKED]
        if blocked:
            return Decision(job, Status.BLOCKED, Cause(Why.DEPENDENCY_BLOCKED, jobs=tuple(blocked)))
        if not job.commands:
            return without_command(dag, job, by_id, seen)
        if any(by_id[dep].status in UPSTREAM for dep in job.depends_on):
            return _below_rerun(dag, job, state, forced, compare, seen)
        decision = judge(dag, job, state, forced, compare, seen)
        if move_files and not forced and decision.status is Status.RUN:
            return _placed(dag, decision, state, compare, seen, claimed, moving)
        return decision

    for job in dag.jobs:
        decision = decide(job)
        if decision.cause.why is not Why.PENDING_SCREEN:
            decision = _with_programs(decision, missing_programs)
        decision = _located(decision, by_id)
        if decision.status is Status.RUN:
            on_disk = any(value is not None for value in stamps(dag, job.output_paths, seen).values())
            decision = replace(decision, replaces=on_disk)
        decisions.append(decision)
        by_id[job.id] = decision
    return tuple(decisions)


def explained(
    dag: Dag, state: Store, decisions: tuple[Decision, ...], job_id: int, compare: Compare
) -> tuple[Decision, Evidence]:
    decision = next((decision for decision in decisions if decision.job.id == job_id), None)
    if decision is None:
        raise DagError(f"no job {job_id} among the jobs planned")
    job = decision.job
    arrived = {
        new: stamp(dag.checked(old), new in dag.folders)
        for each in decisions
        if each.status is Status.PLACE and each.placement is not None
        for old, new in each.placement.moves
    }
    found = evidence(
        dag,
        job,
        state.jobs.get(job.work.key),
        stamps(dag, (*job.input_paths, *job.output_paths), arrived),
        Compare.STAT if compare is Compare.STAT else Compare.CONTENT,
        every=True,
    )
    return decision, found


def writes(decisions: tuple[Decision, ...]) -> tuple[StateWrite, ...]:
    return tuple(write for decision in decisions if decision.status is not Status.PLACE for write in decision.writes)


class AdoptStatus(Enum):
    ADOPTED = "adopted"
    LEFT = "left"


class AdoptCause(Enum):
    NO_COMMAND = "no command"
    OUTPUT_MISSING = "output missing"
    INPUT_MISSING = "input missing"
    OUTPUTS_EXIST = "outputs exist"


@dataclass(frozen=True)
class Adoption:
    job: Job
    status: AdoptStatus
    cause: AdoptCause


def adopt(dag: Dag, state: Store) -> tuple[Adoption, ...]:
    adoptions = []
    for job in dag.jobs:
        inputs = stamps(dag, job.input_paths)
        outputs = stamps(dag, job.output_paths)
        if not job.commands:
            adoption = Adoption(job, AdoptStatus.LEFT, AdoptCause.NO_COMMAND)
        elif any(value is None for value in outputs.values()):
            adoption = Adoption(job, AdoptStatus.LEFT, AdoptCause.OUTPUT_MISSING)
        elif any(value is None for value in inputs.values()):
            adoption = Adoption(job, AdoptStatus.LEFT, AdoptCause.INPUT_MISSING)
        else:
            state.record(job.work.key, records.adopted(job, inputs, outputs))
            adoption = Adoption(job, AdoptStatus.ADOPTED, AdoptCause.OUTPUTS_EXIST)
        adoptions.append(adoption)
    return tuple(adoptions)


def accept_tools(dag: Dag, state: Store) -> int:
    accepted = 0
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is None or record.fingerprint == job.work.fingerprint:
            continue
        changed = changed_parts(record, job)
        if changed and all(isinstance(part, Tool) for part in changed):
            state.record(
                job.work.key, record.model_copy(update={"fingerprint": job.work.fingerprint, "parts": job.work.parts})
            )
            accepted += 1
    return accepted
