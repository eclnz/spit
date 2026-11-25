from __future__ import annotations

from collections import Counter
from enum import Enum

from ..cause import Why
from ..display import id_width, labels, name
from ..model import Dag
from ..plan import Adoption, Decision, Status
from . import LISTED
from .causes import adoption, cause, decided, move


class Kind(Enum):
    START = "start"
    WAIT = "wait"
    MAYBE = "maybe"
    PLACE = "place"
    CHECK = "check"
    BLOCKED = "blocked"
    CURRENT = "current"


def jobs(count: int) -> str:
    return f"{count} job" + ("" if count == 1 else "s")


def _agree(count: int, one: str, many: str) -> str:
    return one if count == 1 else many


def _times(count: int) -> str:
    return f" x{count}" if count > 1 else ""


def _more(hidden: int, what: str, indent: str = "", verbose: bool = True) -> list[str]:
    if hidden <= 0:
        return []
    named = f" {what}" if what else ""
    return [f"{indent}... and {hidden} more{named}" + ("; `--verbose` lists every job" if verbose else "")]


def _kind(decision: Decision) -> Kind:
    match decision.status:
        case Status.RUN:
            return Kind.WAIT if decision.waits_for else Kind.START
        case Status.MAYBE:
            return Kind.MAYBE
        case Status.PLACE:
            return Kind.PLACE
        case Status.CHECK:
            return Kind.CHECK
        case Status.BLOCKED:
            return Kind.BLOCKED
        case Status.SKIP:
            return Kind.CURRENT


def _phrase(kind: Kind, count: int, waits_for: str) -> str:
    match kind:
        case Kind.START:
            return "can start"
        case Kind.WAIT:
            return f"will wait{waits_for}"
        case Kind.MAYBE:
            return "may run if an upstream output changes"
        case Kind.PLACE:
            return f"will move {_agree(count, 'its', 'their')} outputs"
        case Kind.CHECK:
            return f"{_agree(count, 'needs', 'need')} only {_agree(count, 'its', 'their')} checks"
        case Kind.BLOCKED:
            return f"{_agree(count, 'is', 'are')} blocked"
        case Kind.CURRENT:
            return f"{_agree(count, 'is', 'are')} current"


def counts(decisions: tuple[Decision, ...]) -> str:
    total = len(decisions)
    if not total:
        return "no jobs"
    kinds = Counter(_kind(decision) for decision in decisions)
    if kinds[Kind.CURRENT] == total:
        return "1 job is current" if total == 1 else f"all {total} jobs are current"
    upstream = kinds[Kind.START] + kinds[Kind.MAYBE]
    waits_for = "" if not upstream else f" for {_agree(upstream, 'it', 'them')}"
    return ", ".join(f"{jobs(kinds[kind])} {_phrase(kind, kinds[kind], waits_for)}" for kind in Kind if kinds[kind])


def blocked_lines(decisions: tuple[Decision, ...]) -> list[str]:
    held = Counter(decision.caused_by for decision in decisions if decision.caused_by is not None)
    causes: dict[tuple[str, str], list[Decision]] = {}
    for decision in decisions:
        if decision.status is Status.BLOCKED and decision.caused_by is None:
            causes.setdefault((decision.job.operation, cause(decision.cause)), []).append(decision)
    lines = []
    for (operation, reason), members in list(causes.items())[:LISTED]:
        line = f"blocked: {operation}{_times(len(members))}: {reason}"
        more = sum(held[member.job.id] for member in members)
        if more:
            line += f" ({more} more {_agree(more, 'depends', 'depend')} on {_agree(len(members), 'it', 'them')})"
        lines.append(line)
    return lines + _more(len(causes) - LISTED, "causes")


def replacing_lines(decisions: tuple[Decision, ...]) -> list[str]:
    causes = Counter((decision.job.operation, decided(decision)) for decision in decisions if decision.replaces)
    if not causes:
        return []
    lines = [f"{jobs(causes.total())} will replace existing outputs:"]
    lines += [f"  {operation}{_times(count)}: {reason}" for (operation, reason), count in list(causes.items())[:LISTED]]
    return lines + _more(len(causes) - LISTED, "causes", indent="  ")


def _word(decision: Decision) -> str:
    match decision.status:
        case Status.RUN:
            return "waits" if decision.waits_for else "ready"
        case Status.MAYBE:
            return "may run"
        case Status.SKIP:
            return "current"
        case Status.CHECK | Status.BLOCKED | Status.PLACE:
            return decision.status.value


def _reason(decision: Decision) -> str:
    if decision.status is Status.RUN and decision.waits_for:
        waits = decision.waits_for
        return "waits for " + (", ".join(map(str, waits)) if len(waits) <= 3 else f"{len(waits)} jobs")
    return decided(decision)


def job_line(decision: Decision, work: dict[int, str], width: int) -> str:
    job = decision.job
    return f"[{job.id:>{width}}] {_word(decision):7} {name(job, work[job.id])}: {_reason(decision)}"


def not_moved_lines(decisions: tuple[Decision, ...]) -> list[str]:
    refused = [
        (decision.job.id, decision.placement)
        for decision in decisions
        if decision.status is not Status.PLACE and decision.placement is not None
    ]
    lines = [f"[{job_id}] not moved: {move(placement)}" for job_id, placement in refused[:LISTED]]
    return lines + _more(len(refused) - LISTED, "not moved")


def edited_lines(decisions: tuple[Decision, ...]) -> list[str]:
    edited = [
        decision.job
        for decision in decisions
        if decision.status is Status.RUN and decision.cause.why is Why.OUTPUT_CHANGED
    ]
    if not edited:
        return []
    lines = [
        f"{jobs(len(edited))} {_agree(len(edited), 'reruns', 'rerun')} because an output was edited since it was made; "
        "run keeps what is there as NAME.edited-<time> beside it"
    ]
    lines += [f"[{job.id}] edited: {', '.join(job.output_paths)}" for job in edited[:LISTED]]
    return lines + _more(len(edited) - LISTED, "", verbose=False)


def move_hint(movable: int) -> str:
    return (
        f"{jobs(movable)} {_agree(movable, 'reruns', 'rerun')} only because "
        f"{_agree(movable, 'its', 'their')} outputs are recorded at other paths; --move-files moves them instead"
    )


def listing(dag: Dag, decisions: tuple[Decision, ...], verbose: bool) -> list[str]:
    lines = [counts(decisions), *blocked_lines(decisions), *edited_lines(decisions)]
    if verbose:
        work, width = labels(dag.jobs), id_width(dag.jobs)
        lines += [job_line(decision, work, width) for decision in decisions]
    else:
        lines += replacing_lines(decisions)
        lines += not_moved_lines(decisions)
    return lines


def nothing_run(decisions: tuple[Decision, ...]) -> str:
    held = sum(decision.status is Status.BLOCKED for decision in decisions)
    return (
        f"nothing was run: {jobs(held)} {_agree(held, 'is', 'are')} blocked. Fix the cause, or pass "
        "--partial to run the jobs that are not."
    )


def adoption_line(adopted: Adoption) -> str:
    return f"[{adopted.job.id}] {adopted.status.value:7} {adopted.job.operation}: {adoption(adopted.cause)}"
