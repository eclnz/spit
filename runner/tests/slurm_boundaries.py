from __future__ import annotations

from dataclasses import dataclass, replace
from enum import Enum, auto
from typing import Iterator

from slurm_model_search import Model, Step


class Interval(Enum):
    POSITIVE = auto()
    ZERO = auto()
    NONFINITE = auto()


@dataclass(frozen=True)
class Request:
    interval: Interval | None = None
    site_checked: bool = False
    reserved: int = 0
    effective: int = 0
    requeue_allowed: bool = False
    sent: bool = False
    rejected: bool = False


def request_steps(state: Request, fault: str | None) -> Iterator[Step[Request]]:
    if state.sent or state.rejected:
        return
    if state.interval is None:
        for interval in Interval:
            yield Step(f"profile gives {interval.name.lower()} call interval", replace(state, interval=interval))
    elif state.interval is not Interval.POSITIVE and fault != "invalid-interval":
        yield Step("reject profile before any Slurm call", replace(state, rejected=True))
    elif not state.site_checked:
        yield Step("read Slurm site configuration", replace(state, site_checked=True))
    elif not state.reserved:
        yield Step("reserve one task", replace(state, reserved=1))
    elif not state.effective:
        yield Step("build sbatch for one task", replace(state, effective=1))
        yield Step("profile options expand the effective array to two tasks", replace(state, effective=2))
        yield Step("profile options turn requeue back on", replace(state, effective=1, requeue_allowed=True))
    elif state.effective != state.reserved and fault != "unchecked-array":
        yield Step("reject effective command and release unsent reservation", replace(state, rejected=True))
    elif state.requeue_allowed and fault != "requeue-override":
        yield Step("reject effective command that permits requeue", replace(state, rejected=True))
    else:
        yield Step("send effective sbatch command", replace(state, sent=True))


def request_violation(before: Request, step: Step[Request]) -> str | None:
    if not before.site_checked and step.state.site_checked and before.interval is not Interval.POSITIVE:
        return "invalid interval reached the scheduler"
    if step.state.sent:
        if before.effective != before.reserved:
            return "effective task count differs from the reservation"
        if before.requeue_allowed:
            return "effective command allows requeue despite the reservation protocol"
    return None


class Ownership(Enum):
    OURS = auto()
    FOREIGN = auto()


class Proof(Enum):
    EXACT = auto()
    NAME_ONLY = auto()


@dataclass(frozen=True)
class QueueRow:
    owner: Ownership | None = None
    proof: Proof | None = None
    cancelled: bool = False


def ownership_steps(state: QueueRow, fault: str | None) -> Iterator[Step[QueueRow]]:
    if state.cancelled:
        return
    if state.owner is None:
        for owner in (Ownership.FOREIGN, Ownership.OURS):
            yield Step(f"Slurm has a {owner.name.lower()} job with a matching name", replace(state, owner=owner))
    elif state.proof is None:
        yield Step("manifest unavailable; only job name matches", replace(state, proof=Proof.NAME_ONLY))
        if state.owner is Ownership.OURS:
            yield Step("verify immutable submission identity", replace(state, proof=Proof.EXACT))
    elif state.proof is Proof.EXACT or fault == "cancel-on-name":
        yield Step("send scancel for the observed job", replace(state, cancelled=True))


def ownership_violation(before: QueueRow, step: Step[QueueRow]) -> str | None:
    if step.state.cancelled and (before.owner is not Ownership.OURS or before.proof is not Proof.EXACT):
        return "cancellation used a name without proof of ownership"
    return None


class Allocation(Enum):
    LIVE = auto()
    COMPLETING = auto()
    TERMINAL = auto()


@dataclass(frozen=True)
class Monitor:
    allocation: Allocation = Allocation.LIVE
    saw_running: bool = False
    missing: bool = False
    elapsed: int = 0
    queries_left: int = 2
    success_seen: bool = False
    terminal_proven: bool = False
    charged: bool = True
    reported: bool = False


DEADLINE = 2


def monitor_steps(state: Monitor, fault: str | None) -> Iterator[Step[Monitor]]:
    if state.allocation is Allocation.LIVE:
        yield Step("Slurm begins epilog; allocation is still charged", replace(state, allocation=Allocation.COMPLETING))
        if not state.missing and not state.saw_running:
            yield Step("squeue shows RUNNING", replace(state, saw_running=True))
    if state.allocation in (Allocation.LIVE, Allocation.COMPLETING):
        yield Step("Slurm releases allocation", replace(state, allocation=Allocation.TERMINAL))
    if not state.success_seen and state.allocation is not Allocation.TERMINAL:
        charged = fault != "release-on-result"
        yield Step(
            "controller reads success before Slurm releases resources",
            replace(state, success_seen=True, charged=charged),
        )
    if not state.missing and state.charged:
        yield Step("known job ID stops appearing in squeue", replace(state, missing=True))
        if state.allocation is Allocation.TERMINAL:
            yield Step(
                "squeue confirms matching allocation is terminal", replace(state, charged=False, terminal_proven=True)
            )
    if not state.missing or not state.charged or state.reported:
        return
    if state.elapsed < DEADLINE:
        elapsed = state.elapsed + 1
        reported = elapsed == DEADLINE and (state.saw_running or fault != "ignore-unseen-deadline")
        yield Step(
            "reconciliation deadline expires" if elapsed == DEADLINE else "reconciliation time advances",
            replace(state, elapsed=elapsed, reported=reported),
        )
        if state.queries_left:
            yield Step(
                "bounded accounting query also returns no evidence", replace(state, queries_left=state.queries_left - 1)
            )
            if state.allocation is Allocation.TERMINAL:
                yield Step(
                    "accounting proves matching allocation is terminal",
                    replace(state, queries_left=state.queries_left - 1, charged=False, terminal_proven=True),
                )


def monitor_violation(before: Monitor, step: Step[Monitor]) -> str | None:
    state = step.state
    if not state.charged and not state.terminal_proven:
        return "task slot released before proof that Slurm freed the allocation"
    if state.missing and state.elapsed == DEADLINE and state.charged and not state.reported:
        return "missing job ID has no unresolved report at the deadline"
    if before.reported and before.queries_left != state.queries_left:
        return "scheduler queried again after the unresolved report"
    return None


class Attempt(Enum):
    READY = auto()
    WRITTEN = auto()
    SUBMITTED = auto()
    REFUSED = auto()
    BACKOFF = auto()


@dataclass(frozen=True)
class Quota:
    phase: Attempt = Attempt.READY
    attempts: int = 0
    bundles: int = 0


def quota_steps(state: Quota, fault: str | None) -> Iterator[Step[Quota]]:
    if state.phase is Attempt.READY and state.attempts < 2:
        yield Step(
            "persist one manifest, script, and batch", replace(state, phase=Attempt.WRITTEN, bundles=state.bundles + 1)
        )
    elif state.phase is Attempt.WRITTEN:
        yield Step("send one rate-limited sbatch", replace(state, phase=Attempt.SUBMITTED, attempts=state.attempts + 1))
    elif state.phase is Attempt.SUBMITTED:
        yield Step("Slurm definitively refuses at its quota", replace(state, phase=Attempt.REFUSED))
    elif state.phase is Attempt.REFUSED:
        if fault == "retain-refused-intent":
            yield Step("keep the refused attempt's files", replace(state, phase=Attempt.BACKOFF))
        else:
            yield Step("retire refused attempt's files", replace(state, phase=Attempt.BACKOFF, bundles=0))
    elif state.phase is Attempt.BACKOFF:
        yield Step("quota backoff elapses", replace(state, phase=Attempt.READY))
        if fault == "skip-quota-backoff":
            yield Step("retry at the normal submission interval", replace(state, phase=Attempt.WRITTEN, bundles=1))


def quota_violation(before: Quota, step: Step[Quota]) -> str | None:
    if step.state.bundles > 1:
        return "a refused submission accumulated shared-filesystem artifacts"
    if before.phase is Attempt.BACKOFF and step.state.phase is Attempt.WRITTEN:
        return "retry started before the quota backoff elapsed"
    return None


@dataclass(frozen=True)
class Takeover:
    epoch: int = 0
    old_permit: bool = False
    old_in_flight: bool = False
    old_charged: bool = False
    new_charged: bool = False
    new_sent: bool = False


def takeover_steps(state: Takeover, fault: str | None) -> Iterator[Step[Takeover]]:
    if state.epoch == 0:
        if not state.old_charged:
            yield Step(
                "old controller reserves slot and obtains a fenced permit",
                replace(state, old_charged=True, old_permit=True),
            )
        if state.old_permit:
            yield Step(
                "controller pauses before send; replacement revokes unsent permit at the gateway",
                replace(state, epoch=1, old_charged=False),
            )
        if state.old_in_flight:
            charged = fault != "drop-on-takeover"
            yield Step(
                "takeover occurs after old sbatch was sent but before its outcome is known",
                replace(state, epoch=1, old_charged=charged),
            )
    if state.old_permit and (state.epoch == 0 or fault == "stale-permit"):
        yield Step(
            "old controller dispatches sbatch with its cached permit",
            replace(state, old_permit=False, old_in_flight=True),
        )
    if state.epoch and not state.old_charged and not state.new_charged:
        yield Step("replacement reserves the available slot", replace(state, new_charged=True))
    if state.new_charged and not state.new_sent:
        yield Step("replacement dispatches sbatch", replace(state, new_sent=True))
    if state.old_in_flight:
        yield Step(
            "scheduler confirms old request is rejected or terminal",
            replace(state, old_in_flight=False, old_charged=False),
        )


def takeover_violation(before: Takeover, step: Step[Takeover]) -> str | None:
    if before.epoch and not before.old_in_flight and step.state.old_in_flight:
        return "paused controller dispatched with a stale permit after takeover"
    if step.state.old_in_flight and not step.state.old_charged:
        return "takeover released an in-flight submission's reservation"
    if step.state.old_charged and step.state.new_charged:
        return "both controllers reserved the same slot"
    return None


def boundary_models() -> tuple[Model, ...]:
    return (
        Model(
            "effective sbatch request",
            Request(),
            request_steps,
            request_violation,
            ("unchecked-array", "invalid-interval", "requeue-override"),
        ),
        Model("cancellation ownership", QueueRow(), ownership_steps, ownership_violation, ("cancel-on-name",)),
        Model(
            "allocation lifecycle and missing observations",
            Monitor(),
            monitor_steps,
            monitor_violation,
            ("ignore-unseen-deadline", "release-on-result"),
        ),
        Model(
            "quota and intent lifetime",
            Quota(),
            quota_steps,
            quota_violation,
            ("retain-refused-intent", "skip-quota-backoff"),
        ),
        Model(
            "controller takeover and stale permissions",
            Takeover(),
            takeover_steps,
            takeover_violation,
            ("stale-permit", "drop-on-takeover"),
        ),
    )
