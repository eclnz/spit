from __future__ import annotations

import json
from itertools import chain

from ..files import Stamps, stamps
from ..model import Dag
from ..state.record import Record
from ..state.store import StateWrite
from .evidence import Compare, changed, compare_files


def _likeness(key: str) -> tuple[tuple, tuple]:
    made = json.loads(key)
    pairs = tuple(sorted((dimension, value) for _, entities in made for dimension, value in entities))
    return pairs, tuple(sorted(value for _, value in pairs))


def relabel(dag: Dag, records: dict[str, Record], everything: Dag) -> tuple[StateWrite, ...]:
    claimed = {job.work.key for job in everything.jobs}
    orphans: dict[str, list[str]] = {}
    alike: dict[tuple[str, tuple], list[str]] = {}
    for key, record in sorted(records.items()):
        if key not in claimed and record.input_hashes is not None and record.output_hashes is not None:
            orphans.setdefault(record.fingerprint, []).append(key)
            for likeness in _likeness(key):
                alike.setdefault((record.fingerprint, likeness), []).append(key)
    if not orphans:
        return ()
    held: dict[str, str | None] = {}
    seen: Stamps = {}
    writes: list[StateWrite] = []
    taken: set[str] = set()
    handed: set[str] = set()
    for job in dag.jobs:
        key = job.work.key
        fingerprint = job.work.fingerprint
        if key in records or key in taken or not job.commands or not orphans.get(fingerprint):
            continue
        nearest = (old for likeness in _likeness(key) for old in alike.get((fingerprint, likeness), ()))
        for old in dict.fromkeys(chain(nearest, orphans[fingerprint])):
            if old in handed:
                continue
            record = records[old]
            assert record.input_hashes is not None and record.output_hashes is not None
            slots = job.input_paths_by_slot
            if set(slots) != set(record.input_hashes) or changed(
                    compare_files(dag, slots, stamps(dag, job.input_paths, seen), record.inputs, record.input_hashes, Compare.FULL, held)):
                continue
            handed.add(old)
            writes += [StateWrite(old, None), StateWrite(key, record)]
            taken.add(key)
            for slot, path in job.output_paths_by_slot.items():
                held[path] = record.output_hashes.get(slot)
            break
    return tuple(writes)
