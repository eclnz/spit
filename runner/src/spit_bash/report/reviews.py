from __future__ import annotations

from collections.abc import Mapping
from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path

from ..execute.scheduler import Result
from ..model import Dag, Review, Target
from ..plan import Decision, PlanStatus, Status


class Readiness(Enum):
    READY = "ready"
    TO_MAKE = "to make"
    BLOCKED = "blocked"
    FAILED = "failed"
    MISSING = "missing"


@dataclass(frozen=True)
class Waiting:
    target: Target
    file: Path
    readiness: Readiness
    job: int | None


@dataclass(frozen=True)
class Rejected:
    target: Target


ReviewTarget = Waiting | Rejected


@dataclass(frozen=True)
class ReviewReport:
    review: Review
    targets: tuple[ReviewTarget, ...]


def file_of(dag: Dag, target: Target) -> Path:
    base = dag.pipeline_folder if target.in_pipeline else dag.root if target.in_dataset else dag.outputs
    assert base is not None
    return base.joinpath(*target.path.split("/"))


def review_reports(
    dag: Dag, decisions: tuple[Decision, ...], results: tuple[Result, ...] | None = None
) -> list[ReviewReport]:
    if not dag.reviews:
        return []
    makers = {path: job.id for job in dag.jobs for path in job.output_paths}
    decided = {decision.job.id: decision.status for decision in decisions}
    outcomes = {result.job.id: result for result in results or ()}
    after_run = results is not None
    gated = {job.id for job in dag.gated}

    def describe(target: Target) -> ReviewTarget:
        if target.rejected:
            return Rejected(target)
        path = file_of(dag, target)
        job = makers.get(target.path)
        if target.evidence is None:
            return Waiting(target, path, _readiness(path, job, decided, outcomes, after_run), job)
        evidence = dag.path(target.evidence)
        candidate_status = _readiness(path, job, decided, outcomes, after_run)
        jobs = target.evidence_makers or (None,)
        readiness = [
            Readiness.BLOCKED if maker in gated else _readiness(evidence, maker, decided, outcomes, after_run)
            for maker in jobs
        ]
        for status in (Readiness.FAILED, Readiness.BLOCKED, Readiness.TO_MAKE, Readiness.MISSING):
            if status in readiness:
                return Waiting(target, evidence, status, jobs[readiness.index(status)])
            if candidate_status is status:
                return Waiting(target, evidence, status, job)
        return Waiting(target, evidence, Readiness.READY, jobs[0])

    return [ReviewReport(review, tuple(map(describe, review.targets))) for review in dag.reviews]


def _readiness(
    file: Path, job: int | None, decided: Mapping[int, PlanStatus], outcome: dict[int, Result], after_run: bool
) -> Readiness:
    result = outcome.get(job) if job is not None else None
    if result is not None and result.failure is not None:
        return Readiness.BLOCKED if result.caused_by is not None else Readiness.FAILED
    if result is not None and result.settled in (Status.HELD, Status.REJECTED):
        return Readiness.BLOCKED
    status = decided.get(job) if job is not None else None
    if status is Status.BLOCKED:
        return Readiness.BLOCKED
    if not after_run and status is not None and status is not Status.SKIP:
        return Readiness.TO_MAKE
    return Readiness.READY if file.exists() else Readiness.MISSING


def _plural(count: int, word: str) -> str:
    return f"{count} {word}{'' if count == 1 else 's'}"


def _file_line(review: Review, created: set[Path]) -> str:
    if review.file is None:
        return f"  no file holds its verdicts; name one in the recipe with `review {review.name} from FILE`"
    if review.file in created:
        return f"  record verdicts in {review.file}, created by this run"
    if not review.found:
        return f"  record verdicts in {review.file}, which was missing when the runner read it"
    held = "none" if review.verdicts == 0 else str(review.verdicts)
    line = f"  record verdicts in {review.file}, which held {held}"
    if review.unmatched:
        lines = ", ".join(str(number) for number in review.unmatched)
        line += f"; {_plural(len(review.unmatched), 'row')} matched no artifact (line{'s' if len(review.unmatched) > 1 else ''} {lines})"
    return line


def _waiting_line(item: Waiting, after_run: bool) -> str:
    match item.readiness:
        case Readiness.READY:
            return f"  ready to inspect: {item.file}"
        case Readiness.TO_MAKE:
            return f"  to inspect once job {item.job} makes it: {item.file}"
        case Readiness.FAILED:
            return f"  could not be made, as job {item.job} failed: {item.file}"
        case Readiness.BLOCKED:
            return f"  could not be made, as job {item.job} {'did not run' if after_run else 'is blocked'}: {item.file}"
        case Readiness.MISSING:
            maker = f"; job {item.job} makes it" if item.job is not None else ""
            return f"  missing{maker}: {item.file}"


def review_lines(
    reports: list[ReviewReport], *, after_run: bool = False, created: set[Path] | None = None
) -> list[str]:
    lines: list[str] = []
    made = created if created is not None else set()
    for report in reports:
        review = report.review
        if not report.targets and not review.unmatched:
            continue
        rejected = [entry.target for entry in report.targets if isinstance(entry, Rejected)]
        held = len(report.targets) - len(rejected)
        counts = f"{len(rejected)} rejected, {held} held"
        lines.append(f"review `{review.name}`: {counts}")
        lines.append(_file_line(review, made))
        for target in rejected:
            line = f"  rejected {target.identity}" + (f" (line {target.line})" if target.line else "")
            lines.append(line + (f": {target.reason}" if target.reason else ""))
        lines.extend(_waiting_line(entry, after_run) for entry in report.targets if isinstance(entry, Waiting))
    return lines


def _target_json(reviewed: ReviewTarget) -> dict[str, object]:
    target = reviewed.target
    item = reviewed if isinstance(reviewed, Waiting) else None
    return {
        "identity": target.identity,
        "path": target.path,
        "port": None if target.port is None else {"operation": target.port[0], "port": target.port[1]},
        "verdict": "fail" if isinstance(reviewed, Rejected) else None,
        "reason": target.reason,
        "line": target.line,
        "file": None if item is None else str(item.file),
        "readiness": None if item is None else item.readiness.value,
        "job": None if item is None else item.job,
    }


def _report_json(report: ReviewReport) -> dict[str, object]:
    review = report.review
    return {
        "name": review.name,
        "file": None if review.file is None else str(review.file),
        "found": review.found,
        "verdicts": review.verdicts,
        "unmatched": list(review.unmatched),
        "targets": [_target_json(reviewed) for reviewed in report.targets],
        "applied": [asdict(verdict) for verdict in review.applied],
    }


def review_json(dag: Dag, decisions: tuple[Decision, ...]) -> list[dict[str, object]]:
    return [_report_json(report) for report in review_reports(dag, decisions)]
