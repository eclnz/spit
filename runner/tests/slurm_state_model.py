"""Exhaust the interleavings of a bounded Slurm submission protocol.

Two controllers, one distinct intent each, share one task slot and a scheduler
call interval. Each intent represents one array element. The reservation
ledger and rate limiter are shared. Each authorization is an atomic durable
write before the corresponding send. Crashes clear volatile state, including unsent command
authorizations and unrecorded evidence. Requests already sent can outlive a
controller. Only positive evidence for the exact intent establishes rejection
or completion. Smaller models check the boundaries of that protocol: the
effective command, proof of ownership, missing task observations, and the
lifetime of files written for a refused submission. Their invariants cover
those transitions without multiplying their states by every crash ordering.
Scheduler-change models let allocations reactivate, identifiers be reassigned,
and limits change between observation and action.

Run slurm_guarantees.py to check the runner against the corresponding fault
scenarios. Neither script contacts a real scheduler.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, replace
from enum import Enum, auto
from typing import Iterator

from slurm_boundaries import boundary_models
from slurm_model_search import Model, Step, explore
from slurm_scheduler_changes import scheduler_change_models


CALL_BUDGET = 4
TASK_BUDGET = 1


class Phase(Enum):
    NEW = auto()
    RESERVED = auto()
    UNKNOWN = auto()
    ACCEPTED = auto()
    CANCELLING = auto()
    RESOLVED = auto()


CHARGED = {Phase.RESERVED, Phase.UNKNOWN, Phase.ACCEPTED, Phase.CANCELLING}


class Scheduler(Enum):
    UNSENT = auto()
    IN_FLIGHT = auto()
    REJECTED = auto()
    LIVE = auto()
    TERMINAL = auto()


class Evidence(Enum):
    ACCEPTED = auto()
    TERMINAL = auto()
    REJECTED = auto()


class Command(Enum):
    SUBMIT = auto()
    QUERY = auto()
    CANCEL = auto()


class Fault(str, Enum):
    RELEASE_ON_EMPTY = "release-on-empty"
    UNMETERED_QUERY = "unmetered-query"
    CANCEL_FOREIGN = "cancel-foreign"
    FORGET_ON_CRASH = "forget-on-crash"


@dataclass(frozen=True)
class Intent:
    phase: Phase = Phase.NEW
    remaining: int = CALL_BUDGET
    calls: int = 0
    scheduler: Scheduler = Scheduler.UNSENT
    up: bool = True
    authorized: Command | None = None
    awaiting_submit: bool = False
    awaiting_poll: bool = False
    evidence: Evidence | None = None


@dataclass(frozen=True)
class State:
    intents: tuple[Intent, Intent] = (Intent(), Intent())
    cooling_down: bool = False

    def update(self, owner: int, **changes) -> State:
        intent = replace(self.intents[owner], **changes)
        return replace(self, intents=(intent, self.intents[1]) if owner == 0 else (self.intents[0], intent))


@dataclass(frozen=True)
class SubmissionStep(Step[State]):
    call: bool = False
    cancellation: tuple[int, int] | None = None
    crash: bool = False


def transitions(state: State, fault: str | None = None) -> Iterator[SubmissionStep]:
    if state.cooling_down:
        yield SubmissionStep("scheduler call interval elapses", replace(state, cooling_down=False))
    reserved = sum(task.phase in CHARGED for task in state.intents)
    for owner, task in enumerate(state.intents):
        prefix = "AB"[owner]

        def action(label: str, **changes) -> SubmissionStep:
            return SubmissionStep(f"{prefix}: {label}", state.update(owner, **changes))

        def authorize(label: str, command: Command, **changes) -> SubmissionStep:
            return action(label, authorized=command, remaining=max(0, task.remaining - 1), **changes)

        def send(label: str, *, target: int | None = None, **changes) -> SubmissionStep:
            updated = state.update(owner, calls=task.calls + 1, authorized=None, **changes)
            cancellation = None if target is None else (owner, target)
            return SubmissionStep(f"{prefix}: {label}", replace(updated, cooling_down=True),
                                  call=True, cancellation=cancellation)

        if task.scheduler is Scheduler.IN_FLIGHT:
            yield action("Slurm accepts submit", scheduler=Scheduler.LIVE)
            yield action("Slurm rejects submit", scheduler=Scheduler.REJECTED)
        if task.scheduler is Scheduler.LIVE:
            yield action("Slurm task becomes terminal", scheduler=Scheduler.TERMINAL)

        if not task.up:
            yield action("restart", up=True)
            continue
        crashed = state.update(owner, up=False, authorized=None, awaiting_submit=False, awaiting_poll=False,
                               evidence=None)
        if fault == Fault.FORGET_ON_CRASH and task.phase is Phase.UNKNOWN:
            crashed = crashed.update(owner, phase=Phase.NEW, remaining=CALL_BUDGET)
        yield SubmissionStep(f"{prefix}: crash", crashed, crash=True)

        if task.phase is Phase.NEW and reserved < TASK_BUDGET:
            yield action("persist intent and reserve slot", phase=Phase.RESERVED)
        can_authorize = task.authorized is None and task.remaining > 0
        if task.phase is Phase.RESERVED and can_authorize:
            yield authorize("persist unknown acceptance and debit sbatch", Command.SUBMIT, phase=Phase.UNKNOWN)
        if task.authorized is Command.SUBMIT and not state.cooling_down:
            yield send("send sbatch", awaiting_submit=True, scheduler=Scheduler.IN_FLIGHT)

        if task.awaiting_submit:
            yield action("lose sbatch reply", awaiting_submit=False)
            if task.scheduler in (Scheduler.LIVE, Scheduler.TERMINAL):
                yield action("receive sbatch acceptance", awaiting_submit=False, evidence=Evidence.ACCEPTED)
            if task.scheduler is Scheduler.REJECTED:
                yield action("receive sbatch rejection", awaiting_submit=False, evidence=Evidence.REJECTED)

        unmetered = fault == Fault.UNMETERED_QUERY
        if task.phase in (Phase.UNKNOWN, Phase.ACCEPTED, Phase.CANCELLING) and not task.awaiting_poll:
            if can_authorize or (unmetered and task.authorized is None):
                yield authorize("debit squeue or accounting query", Command.QUERY)
        if task.authorized is Command.QUERY and (not state.cooling_down or unmetered):
            yield send("send squeue or accounting query", awaiting_poll=True)
        if task.awaiting_poll:
            yield action("query fails or its reply is lost", awaiting_poll=False)
            if fault == Fault.RELEASE_ON_EMPTY and task.phase in CHARGED:
                yield action("empty snapshot releases slot", awaiting_poll=False, phase=Phase.RESOLVED)
            else:
                yield action("empty or stale snapshot leaves slot charged", awaiting_poll=False)
            if task.scheduler in (Scheduler.LIVE, Scheduler.TERMINAL):
                yield action("query sees task of this intent (possibly stale)", awaiting_poll=False,
                             evidence=Evidence.ACCEPTED)
            if task.scheduler is Scheduler.TERMINAL:
                yield action("query confirms terminal state for this intent", awaiting_poll=False,
                             evidence=Evidence.TERMINAL)
        if task.evidence is Evidence.ACCEPTED and task.phase is Phase.UNKNOWN:
            yield action("persist acceptance", phase=Phase.ACCEPTED, evidence=None)
        if task.evidence in (Evidence.REJECTED, Evidence.TERMINAL):
            yield action("persist resolution and release slot", phase=Phase.RESOLVED, evidence=None, authorized=None,
                         awaiting_submit=False, awaiting_poll=False)

        if can_authorize and task.phase is Phase.ACCEPTED:
            yield authorize("persist cancellation and debit scancel", Command.CANCEL, phase=Phase.CANCELLING)
        if task.authorized is Command.CANCEL and not state.cooling_down:
            yield send("send scancel for own recorded identity", target=owner)
        other = state.intents[1 - owner]
        if can_authorize and not state.cooling_down and fault == Fault.CANCEL_FOREIGN and other.scheduler is Scheduler.LIVE:
            yield send(f"cancel {'AB'[1 - owner]}'s job by shared name", target=1 - owner, remaining=task.remaining - 1)


def violation(before: State, step: SubmissionStep) -> str | None:
    tasks = step.state.intents
    if sum(task.phase in CHARGED for task in tasks) > TASK_BUDGET:
        return "task budget exceeded"
    if sum(task.scheduler is Scheduler.LIVE for task in tasks) > TASK_BUDGET:
        return "Slurm has more live tasks than the budget permits"
    if any(task.phase not in CHARGED and task.scheduler in (Scheduler.IN_FLIGHT, Scheduler.LIVE) for task in tasks):
        return "a pending or live submission lost its reservation"
    if any(task.phase is Phase.RESOLVED and task.scheduler not in (Scheduler.REJECTED, Scheduler.TERMINAL)
           for task in tasks):
        return "reservation released without proof of rejection or completion"
    if (step.call and before.cooling_down) or any(task.calls + task.remaining > CALL_BUDGET for task in tasks):
        return "scheduler traffic exceeded its call budget or spacing"
    if step.cancellation and step.cancellation[0] != step.cancellation[1]:
        return "cancellation targeted another invocation"
    if step.crash and any((old.phase, old.remaining) != (new.phase, new.remaining)
                          for old, new in zip(before.intents, tasks)):
        return "crash erased durable intent or call budget"
    return None


def main() -> int:
    models = (Model("submit and recover", State(), transitions, violation, tuple(fault.value for fault in Fault)),
              *boundary_models(), *scheduler_change_models())
    by_fault = {fault: model for model in models for fault in model.faults}
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fault", choices=by_fault, metavar="FAULT",
                        help="emit a shortest counterexample for " + ", ".join(by_fault))
    args = parser.parse_args()
    for model in (models if args.fault is None else (by_fault[args.fault],)):
        result = explore(model, args.fault)
        if result.problem:
            print(f"FAIL {model.name}: {result.problem}")
            for number, action in enumerate(result.trace, 1):
                print(f"  {number}. {action}")
            return 1
        if args.fault is None:
            print(f"PASS {model.name}: {result.states:,} states and {result.edges:,} transitions")
    if args.fault is not None:
        print(f"FAIL explorer missed {args.fault}")
        return 1
    for fault, model in by_fault.items():
        broken = explore(model, fault)
        if not broken.problem:
            print(f"FAIL explorer missed {fault}")
            return 1
        print(f"PASS {fault} produces a counterexample at depth {len(broken.trace)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
