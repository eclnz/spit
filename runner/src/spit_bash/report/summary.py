from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass

from ..execute.scheduler import Result
from ..files import stamps
from ..model import Dag, Job
from ..plan import Decision, Status
from ..reuse.evidence import Compare, compare_files
from ..state.record import Record
from ..state.store import Store


@dataclass(frozen=True)
class Summary:
    done: int
    placed: int
    checked: int
    failed: tuple[Result, ...]
    not_run: tuple[Result, ...]
    current: int
    blocked: int
    kept: int

    @classmethod
    def of(cls, decisions: tuple[Decision, ...], results: tuple[Result, ...]) -> Summary:
        failures = [result for result in results if result.failure is not None]
        failed_ids = {result.job.id for result in failures}
        outcome = {result.job.id: result.settled for result in results}
        count = {status: 0 for status in Status}
        kept = 0
        for decision in decisions:
            status = outcome.get(decision.job.id) or decision.status
            if decision.job.id in failed_ids:
                continue
            if decision.status is Status.MAYBE and status is Status.SKIP:
                kept += 1
                continue
            count[status] += 1
        return cls(
            done=count[Status.RUN],
            placed=count[Status.PLACE],
            checked=count[Status.CHECK],
            failed=tuple(result for result in failures if result.caused_by is None),
            not_run=tuple(result for result in failures if result.caused_by is not None),
            current=count[Status.SKIP],
            blocked=count[Status.BLOCKED],
            kept=kept,
        )


@dataclass(frozen=True)
class Left:
    stale: int
    unchanged: int


def left_out(
    everything: Dag,
    chosen: Dag,
    decisions: tuple[Decision, ...],
    results: tuple[Result, ...],
    state: Store,
    compare: Compare,
) -> Left:
    hashes = rewritten_hashes(decisions, results, state)
    stale = 0
    read: set[str] = set()
    changed: set[str] = set()
    for job in jobs_left_out(everything, chosen):
        rewritten = rewritten_inputs(job, hashes)
        if not rewritten:
            continue
        read.update(rewritten.values())
        record = state.jobs.get(job.work.key)
        if record is None:
            stale += 1
            continue
        differing = changed_inputs(everything, rewritten, record, hashes, compare)
        changed.update(differing)
        stale += bool(differing)
    return Left(stale, len(read - changed))


def rewritten_hashes(
    decisions: tuple[Decision, ...], results: tuple[Result, ...], state: Store
) -> dict[str, str | None]:
    settled = {result.job.id: result.settled for result in results if result.failure is None}
    hashes: dict[str, str | None] = {}
    for decision in decisions:
        if ran(decision, settled):
            hashes.update(output_hashes(decision.job, state))
    return hashes


def ran(decision: Decision, settled: dict[int, Status | None]) -> bool:
    job_id = decision.job.id
    return job_id in settled and (settled[job_id] or decision.status) is Status.RUN


def output_hashes(job: Job, state: Store) -> dict[str, str | None]:
    record = state.jobs.get(job.work.key)
    held = record.output_hashes if record is not None and record.output_hashes else {}
    return {path: held.get(slot) for slot, path in job.output_paths_by_slot.items()}


def jobs_left_out(everything: Dag, chosen: Dag) -> Iterator[Job]:
    kept = {job.id for job in chosen.jobs}
    return (job for job in everything.jobs if job.id not in kept)


def rewritten_inputs(job: Job, hashes: dict[str, str | None]) -> dict[str, str]:
    return {slot: path for slot, path in job.input_paths_by_slot.items() if path in hashes}


def changed_inputs(
    everything: Dag,
    rewritten: dict[str, str],
    record: Record,
    hashes: dict[str, str | None],
    compare: Compare,
) -> set[str]:
    known = {path: hashes[path] for path in rewritten.values() if hashes[path] is not None}
    now = stamps(everything, tuple(rewritten.values()))
    diffs = compare_files(everything, rewritten, now, record.inputs, record.input_hashes, compare, known, True)
    return {diff.path for diff in diffs if diff.changed}
