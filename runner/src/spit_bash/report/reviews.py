from __future__ import annotations

from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path

from ..execute.scheduler import Result
from ..model import Dag, Review, Target
from ..plan import Decision, Status


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


def file_of(dag: Dag, target: Target) -> Path:
    base = dag.pipeline_folder if target.in_pipeline else dag.root if target.in_dataset else dag.outputs
    assert base is not None
    return base.joinpath(*target.path.split("/"))


def waiting(dag: Dag, review: Review, decisions: tuple[Decision, ...],
            results: tuple[Result, ...] | None = None) -> list[Waiting]:
    makers = {path: job.id for job in dag.jobs for path in job.output_paths}
    decided = {decision.job.id: decision.status for decision in decisions}
    outcome = {result.job.id: result for result in results or ()}
    found = []
    for target in review.targets:
        if target.rejected:
            continue
        file = file_of(dag, target)
        job = makers.get(target.path)
        found.append(Waiting(target, file, _readiness(file, job, decided, outcome, results is not None), job))
    return found


def _readiness(file: Path, job: int | None, decided: dict[int, Status], outcome: dict[int, Result],
               after_run: bool) -> Readiness:
    result = outcome.get(job) if job is not None else None
    if result is not None and result.failure is not None:
        return Readiness.BLOCKED if result.caused_by is not None else Readiness.FAILED
    status = decided.get(job) if job is not None else None
    if status is Status.BLOCKED:
        return Readiness.BLOCKED
    if not after_run and status is not None and status is not Status.SKIP:
        return Readiness.TO_MAKE
    return Readiness.READY if file.exists() else Readiness.MISSING


def _plural(count: int, word: str) -> str:
    return f"{count} {word}{'' if count == 1 else 's'}"


def _file_line(review: Review) -> str:
    if review.file is None:
        return f"  no file holds its verdicts; name one in the recipe with `review {review.name} from FILE`"
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


def review_lines(dag: Dag, decisions: tuple[Decision, ...], results: tuple[Result, ...] | None = None) -> list[str]:
    lines: list[str] = []
    after_run = results is not None
    for review in dag.reviews:
        if not review.targets and not review.unmatched:
            continue
        rejected = [target for target in review.targets if target.rejected]
        held = len(review.targets) - len(rejected)
        counts = f"{len(rejected)} rejected, {held} held"
        lines.append(f"review `{review.name}`: {counts}")
        lines.append(_file_line(review))
        for target in rejected:
            line = f"  rejected {target.identity}" + (f" (line {target.line})" if target.line else "")
            lines.append(line + (f": {target.reason}" if target.reason else ""))
        lines.extend(_waiting_line(item, after_run) for item in waiting(dag, review, decisions, results))
    return lines


def review_json(dag: Dag, decisions: tuple[Decision, ...]) -> list[dict[str, object]]:
    reviews: list[dict[str, object]] = []
    for review in dag.reviews:
        held = iter(waiting(dag, review, decisions))
        targets = []
        for target in review.targets:
            item = None if target.rejected else next(held)
            targets.append({
                "identity": target.identity, "path": target.path,
                "port": None if target.port is None else {"operation": target.port[0], "port": target.port[1]},
                "verdict": "fail" if target.rejected else None, "reason": target.reason, "line": target.line,
                "file": None if item is None else str(item.file),
                "readiness": None if item is None else item.readiness.value,
                "job": None if item is None else item.job,
            })
        reviews.append({"name": review.name, "file": None if review.file is None else str(review.file),
                        "found": review.found, "verdicts": review.verdicts, "unmatched": list(review.unmatched),
                        "targets": targets,
                        "applied": [asdict(verdict) for verdict in review.applied]})
    return reviews
