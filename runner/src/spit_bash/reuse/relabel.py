from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from enum import Enum
from itertools import chain

from ..files import Stamps, stamps
from ..model import Dag, Job
from ..state.record import Record
from ..state.store import StateWrite
from .evidence import Compare, changed, compare_files
from .identity import EntityPairs, claimed_keys, decode_key


class Alike(Enum):
    ENTITIES = "entities"
    VALUES = "values"


@dataclass(frozen=True)
class Likeness:
    fingerprint: str
    alike: Alike
    key: EntityPairs | tuple[str, ...]


@dataclass(frozen=True)
class Orphans:
    by_fingerprint: dict[str, list[str]]
    by_likeness: dict[Likeness, list[str]]


def relabel(dag: Dag, records: dict[str, Record], everything: Dag) -> tuple[StateWrite, ...]:
    orphans = _orphans(records, claimed_keys(everything))
    if not orphans.by_fingerprint:
        return ()
    held: dict[str, str | None] = {}
    seen: Stamps = {}
    writes: list[StateWrite] = []
    taken: set[str] = set()
    handed: set[str] = set()
    for job in dag.jobs:
        key = job.work.key
        if not _needs_record(job, records, taken, orphans):
            continue
        old = _first_match(dag, job, records, _candidates(orphans, job), handed, seen, held)
        if old is None:
            continue
        record = records[old]
        handed.add(old)
        writes += [StateWrite(old, None), StateWrite(key, record)]
        taken.add(key)
        held.update(_held_hashes(job, record))
    return tuple(writes)


def _reusable(record: Record) -> bool:
    return record.input_hashes is not None and record.output_hashes is not None


def _likenesses(fingerprint: str, key: str) -> tuple[Likeness, Likeness]:
    made = decode_key(key)
    pairs = tuple(sorted((dimension, value) for _, entities in made for dimension, value in entities))
    values = tuple(sorted(value for _, value in pairs))
    return Likeness(fingerprint, Alike.ENTITIES, pairs), Likeness(fingerprint, Alike.VALUES, values)


def _orphans(records: dict[str, Record], claimed: set[str]) -> Orphans:
    orphans = Orphans({}, {})
    for key, record in sorted(records.items()):
        if key in claimed or not _reusable(record):
            continue
        orphans.by_fingerprint.setdefault(record.fingerprint, []).append(key)
        for likeness in _likenesses(record.fingerprint, key):
            orphans.by_likeness.setdefault(likeness, []).append(key)
    return orphans


def _needs_record(job: Job, records: dict[str, Record], taken: set[str], orphans: Orphans) -> bool:
    key = job.work.key
    unrecorded = key not in records and key not in taken
    return unrecorded and bool(job.commands) and bool(orphans.by_fingerprint.get(job.work.fingerprint))


def _candidates(orphans: Orphans, job: Job) -> Iterable[str]:
    fingerprint = job.work.fingerprint
    likenesses = _likenesses(fingerprint, job.work.key)
    nearest = (old for likeness in likenesses for old in orphans.by_likeness.get(likeness, ()))
    return dict.fromkeys(chain(nearest, orphans.by_fingerprint[fingerprint]))


def _first_match(
    dag: Dag,
    job: Job,
    records: dict[str, Record],
    candidates: Iterable[str],
    handed: set[str],
    seen: Stamps,
    held: dict[str, str | None],
) -> str | None:
    for old in candidates:
        if old not in handed and _same_inputs(dag, job, records[old], seen, held):
            return old
    return None


def _same_inputs(dag: Dag, job: Job, record: Record, seen: Stamps, held: dict[str, str | None]) -> bool:
    assert record.input_hashes is not None, "an orphaned record has input hashes"
    slots = job.input_paths_by_slot
    if set(slots) != set(record.input_hashes):
        return False
    now = stamps(dag, job.input_paths, seen)
    return not changed(compare_files(dag, slots, now, record.inputs, record.input_hashes, Compare.FULL, held))


def _held_hashes(job: Job, record: Record) -> dict[str, str | None]:
    assert record.output_hashes is not None, "an orphaned record has output hashes"
    return {path: record.output_hashes.get(slot) for slot, path in job.output_paths_by_slot.items()}
