from __future__ import annotations

from dataclasses import dataclass, replace
from enum import Enum, auto
from typing import Iterator

from slurm_model_search import Model, Step


class OldJob(Enum):
    LIVE = auto()
    DORMANT = auto()
    RETIRED = auto()


@dataclass(frozen=True)
class Capacity:
    old: OldJob = OldJob.LIVE
    old_width: int = 0
    retired_elements: int = 0
    observed_retired_elements: int = 0
    saw_missing_element: bool = False
    released_elements: int = 0
    observed: OldJob | None = None
    limit: int = 2
    observed_limit: int = 2
    prepared: int = 0
    sent: int = 0


def capacity_steps(state: Capacity, fault: str | None) -> Iterator[Step[Capacity]]:
    if not state.old_width:
        for width in (1, 2):
            yield Step(f"recover an incumbent array of {width} task(s)", replace(state, old_width=width))
        return

    if state.old is OldJob.LIVE:
        yield Step("old job becomes terminal-looking but can requeue", replace(state, old=OldJob.DORMANT))
    if state.old is OldJob.DORMANT:
        yield Step("old job is reactivated", replace(state, old=OldJob.LIVE))
    if state.old is not OldJob.RETIRED:
        yield Step(
            "scheduler retires every remaining element",
            replace(state, old=OldJob.RETIRED, retired_elements=state.old_width),
        )
    if state.retired_elements + 1 < state.old_width:
        yield Step(
            "one array element retires while its sibling can still run",
            replace(state, retired_elements=state.retired_elements + 1),
        )
    if state.old is not state.observed:
        yield Step(
            f"controller observes old submission as {state.old.name.lower()}", replace(state, observed=state.old)
        )
    if state.observed_retired_elements < state.retired_elements:
        yield Step(
            f"verify retirement of {state.retired_elements} array element(s)",
            replace(state, observed_retired_elements=state.retired_elements),
        )
    if state.old_width == 2 and not state.saw_missing_element:
        yield Step("expanded queue reply omits one array element", replace(state, saw_missing_element=True))
    if state.saw_missing_element and state.released_elements == 0 and fault == "missing-element-release":
        yield Step("treat the omitted element as retired", replace(state, released_elements=1))
    if state.released_elements < state.observed_retired_elements:
        released = state.old_width if fault == "partial-array-release" else state.observed_retired_elements
        label = (
            "release the whole array from a partial terminal report"
            if released > state.observed_retired_elements
            else "release only the proven retired elements"
        )
        yield Step(label, replace(state, released_elements=released))
    if (
        state.released_elements < state.old_width
        and state.observed is OldJob.DORMANT
        and fault == "requeueable-release"
    ):
        yield Step(
            "release reservation on a terminal-looking report", replace(state, released_elements=state.old_width)
        )

    if state.limit > 0:
        yield Step(f"lower task limit to {state.limit - 1}", replace(state, limit=state.limit - 1))
    if state.observed_limit != state.limit:
        yield Step("controller discovers the lower limit", replace(state, observed_limit=state.limit))

    charged = state.old_width - state.released_elements
    if not state.prepared:
        for width in (1, 2):
            if charged + width <= state.observed_limit:
                yield Step(f"prepare {width} new task(s) using the observed limit", replace(state, prepared=width))
    if state.prepared and not state.sent:
        enforced_limit = state.observed_limit if fault == "stale-limit" else state.limit
        if charged + state.prepared <= enforced_limit:
            yield Step(f"dispatch {state.prepared} new task(s)", replace(state, sent=state.prepared))


def capacity_violation(before: Capacity, step: Step[Capacity]) -> str | None:
    if not before.sent and step.state.sent:
        possible = before.old_width - before.retired_elements
        total = possible + step.state.sent
        if total > before.limit:
            return f"dispatch can occupy {total} slots under a limit of {before.limit}, counting work that can return"
    return None


class Target(Enum):
    ORIGINAL = auto()
    DORMANT = auto()
    GONE = auto()
    REPLACEMENT = auto()
    SIBLING = auto()


class Cancellation(Enum):
    NONE = auto()
    SENT = auto()
    ACCEPTED = auto()
    REJECTED = auto()


@dataclass(frozen=True)
class DelayedCancel:
    target: Target = Target.ORIGINAL
    original_verified: bool = False
    request: Cancellation = Cancellation.NONE
    other_cluster_has_id: bool = False
    live_sibling: bool = False
    terminal_verified: bool = False
    reported_done: bool = False


@dataclass(frozen=True)
class CancelStep(Step[DelayedCancel]):
    affected: Target | None = None


def cancellation_steps(state: DelayedCancel, fault: str | None) -> Iterator[CancelStep]:
    if not state.other_cluster_has_id:
        yield CancelStep(
            "another cluster has unrelated work under the same numeric ID", replace(state, other_cluster_has_id=True)
        )
    if not state.live_sibling:
        yield CancelStep("an unselected sibling is live in the same array", replace(state, live_sibling=True))
    if state.target is Target.ORIGINAL:
        if not state.original_verified:
            yield CancelStep(
                "verify the original identity at its numeric job ID", replace(state, original_verified=True)
            )
        yield CancelStep("original job looks terminal but can return", replace(state, target=Target.DORMANT))
    if state.target is Target.DORMANT:
        yield CancelStep("original job is reactivated", replace(state, target=Target.ORIGINAL))
    if state.target in (Target.ORIGINAL, Target.DORMANT):
        yield CancelStep("original job is irrevocably retired", replace(state, target=Target.GONE))
    if state.target is Target.GONE:
        yield CancelStep("numeric job ID is reassigned to unrelated work", replace(state, target=Target.REPLACEMENT))
    if state.original_verified and state.request is Cancellation.NONE and state.target is Target.ORIGINAL:
        yield CancelStep("send a cancellation; it remains in flight", replace(state, request=Cancellation.SENT))
    if state.request is Cancellation.SENT:
        if state.live_sibling and state.target is Target.ORIGINAL and fault == "widen-cancel-to-array":
            yield CancelStep(
                "a cancellation filter widens the element ID to the whole array",
                replace(state, request=Cancellation.ACCEPTED),
                affected=Target.SIBLING,
            )
        if state.other_cluster_has_id and fault == "drop-cluster-qualifier":
            yield CancelStep(
                "unqualified cancellation affects a same-ID job on the other cluster",
                replace(state, request=Cancellation.ACCEPTED),
                affected=Target.REPLACEMENT,
            )
        if state.target in (Target.ORIGINAL, Target.DORMANT) or (
            state.target is Target.REPLACEMENT and fault == "id-only-cancel"
        ):
            yield CancelStep(
                "delayed cancellation takes effect at the scheduler",
                replace(state, request=Cancellation.ACCEPTED),
                affected=state.target,
            )
        else:
            yield CancelStep(
                "scheduler rejects cancellation because the original identity is gone",
                replace(state, request=Cancellation.REJECTED),
            )
    if state.request is Cancellation.ACCEPTED and not state.reported_done:
        if state.target in (Target.GONE, Target.REPLACEMENT) and not state.terminal_verified:
            yield CancelStep(
                "receive terminal proof for the original submission", replace(state, terminal_verified=True)
            )
        if state.terminal_verified:
            yield CancelStep("report cancellation complete", replace(state, reported_done=True))
        elif fault == "report-cancel-on-ack":
            yield CancelStep(
                "report cancellation complete from its acknowledgement", replace(state, reported_done=True)
            )


def cancellation_violation(before: DelayedCancel, step: CancelStep) -> str | None:
    if step.affected is Target.REPLACEMENT:
        return "cancellation affected unrelated work with the same numeric ID"
    if step.affected is Target.SIBLING:
        return "cancellation affected an unselected array element"
    if not before.reported_done and step.state.reported_done and not before.terminal_verified:
        return "reported completed cancellation without terminal proof"
    return None


CALLS_PER_WINDOW = 2
WORK_PER_WINDOW = 3


@dataclass(frozen=True)
class Traffic:
    actual: tuple[tuple[int, ...], tuple[int, ...]] = ((), ())
    charged: tuple[tuple[int, ...], tuple[int, ...]] = ((), ())
    restarted: bool = False


def traffic_steps(state: Traffic, fault: str | None) -> Iterator[Step[Traffic]]:
    if any(state.actual) or any(state.charged):
        yield Step(
            "one tick elapses in the rolling window",
            replace(state, actual=(state.actual[1], ()), charged=(state.charged[1], ())),
        )
    if not state.restarted:
        charged = ((), ()) if fault == "reset-query-budget" else state.charged
        yield Step(
            "controller restarts within the same rolling window", replace(state, restarted=True, charged=charged)
        )
    earlier, current = state.charged
    if len(earlier) + len(current) == CALLS_PER_WINDOW:
        return
    for scope, upper, costs in (
        ("job-specific", 1, (1,)),
        ("bounded array", 3, (1, 3)),
        ("unbounded expanded array", None, (1, 4)),
    ):
        if upper is None and fault != "count-only-query-budget":
            continue
        charged_work = 1 if upper is None else upper
        if sum(earlier + current) + charged_work > WORK_PER_WINDOW and fault != "count-only-query-budget":
            continue
        for cost in costs:
            actual = (state.actual[0], state.actual[1] + (cost,))
            yield Step(
                f"dispatch {scope} query costing {cost} work unit(s)",
                replace(state, charged=(earlier, current + (charged_work,)), actual=actual),
            )


def traffic_violation(before: Traffic, step: Step[Traffic]) -> str | None:
    calls = step.state.actual[0] + step.state.actual[1]
    if len(calls) > CALLS_PER_WINDOW:
        return "scheduler call count exceeded the rolling-window allowance"
    if sum(calls) > WORK_PER_WINDOW:
        return "queries exceeded the server-work allowance"
    return None


def scheduler_change_models() -> tuple[Model, ...]:
    return (
        Model(
            "reactivation, partial arrays and changing limits",
            Capacity(),
            capacity_steps,
            capacity_violation,
            ("requeueable-release", "partial-array-release", "missing-element-release", "stale-limit"),
        ),
        Model(
            "delayed cancellation and job identity",
            DelayedCancel(),
            cancellation_steps,
            cancellation_violation,
            ("id-only-cancel", "drop-cluster-qualifier", "report-cancel-on-ack", "widen-cancel-to-array"),
        ),
        Model(
            "rolling scheduler call and work budgets",
            Traffic(),
            traffic_steps,
            traffic_violation,
            ("count-only-query-budget", "reset-query-budget"),
        ),
    )
