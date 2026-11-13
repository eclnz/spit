"""What the DAG's reviews withheld, and whether each artifact waiting for a verdict is ready to inspect."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from ..execute.scheduler import Result
from ..model import Dag, Review, Target
from ..plan import Decision, Status


class Readiness(Enum):
    # The artifact is on disk and no job will rewrite it: a person can inspect it now.
    READY = "ready"
    # A job of this plan makes it first.
    TO_MAKE = "to make"
    # The job that makes it is blocked, or did not run because a job it needs failed.
    BLOCKED = "blocked"
    # The job that makes it failed.
    FAILED = "failed"
    # It is not on disk, and no job of this plan makes it.
    MISSING = "missing"


@dataclass(frozen=True)
class Waiting:
    """A held artifact, where it is, and whether a person can inspect it."""

    target: Target
    file: Path
    readiness: Readiness
    # The job that makes it, when the DAG has one.
    job: int | None


def file_of(dag: Dag, target: Target) -> Path:
    """Where a target is on disk: under the dataset root or the output root, as the DAG says."""
    base = dag.root if target.in_dataset else dag.outputs
    return base.joinpath(*target.path.split("/"))


def waiting(dag: Dag, review: Review, decisions: tuple[Decision, ...],
            results: tuple[Result, ...] | None = None) -> list[Waiting]:
    """Each artifact `review` holds, judged by `decisions` before a run, or by `results` after it.

    `dag` is the whole DAG, so a job a selection left out still names what makes an artifact.
    """
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
        return f"  record verdicts in {review.file}, which did not exist when the DAG was made"
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
    """Each review that withheld anything: what it rejected and held, what that kept from the plan, where its
    verdicts go, and each held artifact with whether it can be inspected.

    Withheld work is no failure: it waits for a verdict, and the DAG made again once one is recorded plans it.
    """
    lines: list[str] = []
    after_run = results is not None
    for review in dag.reviews:
        if not review.targets:
            continue
        rejected = [target for target in review.targets if target.rejected]
        held = len(review.targets) - len(rejected)
        withheld = {output for target in review.targets for output in target.withholds}
        shrunk = {output for target in review.targets for output in target.shrinks} - withheld
        counts = f"{len(rejected)} rejected, {held} held; {_plural(len(withheld), 'output')} withheld"
        if shrunk:
            counts += f", {_plural(len(shrunk), 'output')} made without them"
        lines.append(f"review `{review.name}`: {counts}")
        lines.append(_file_line(review))
        for target in rejected:
            line = f"  rejected {target.identity}" + (f" (line {target.line})" if target.line else "")
            lines.append(line + (f": {target.reason}" if target.reason else ""))
        lines.extend(_waiting_line(item, after_run) for item in waiting(dag, review, decisions, results))
    return lines


def review_json(dag: Dag, decisions: tuple[Decision, ...]) -> list[dict[str, object]]:
    """The DAG's reviews as `plan --json` gives them, each held artifact with its readiness."""
    reviews: list[dict[str, object]] = []
    for review in dag.reviews:
        # `waiting` gives the held targets in order, so each takes the next.
        held = iter(waiting(dag, review, decisions))
        targets = []
        for target in review.targets:
            item = None if target.rejected else next(held)
            targets.append({
                "identity": target.identity, "path": target.path,
                "port": None if target.port is None else {"operation": target.port[0], "port": target.port[1]},
                "verdict": "fail" if target.rejected else None, "reason": target.reason, "line": target.line,
                "withholds": list(target.withholds), "shrinks": list(target.shrinks),
                "file": None if item is None else str(item.file),
                "readiness": None if item is None else item.readiness.value,
                "job": None if item is None else item.job,
            })
        reviews.append({"name": review.name, "file": None if review.file is None else str(review.file),
                        "found": review.found, "verdicts": review.verdicts, "unmatched": list(review.unmatched),
                        "targets": targets})
    return reviews
