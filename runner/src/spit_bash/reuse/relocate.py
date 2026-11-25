from __future__ import annotations

import os
from dataclasses import dataclass
from enum import Enum

from ..cause import Changed
from ..files import Stamps, stamp, stamps, stored
from ..model import Dag, DagError, Job
from ..state.record import Record
from .evidence import Compare, changed, changed_parts, compare_files


class MoveCause(Enum):
    MOVABLE = "movable"
    WORK_CHANGED = "work changed"
    INPUT_CHANGED = "input changed"
    PATH_UNRECORDED = "path unrecorded"
    PARTLY_IN_PLACE = "partly in place"
    BAD_PATH = "bad path"
    DESTINATION_EXISTS = "destination exists"
    STILL_USED = "still used"
    OUTPUT_CHANGED = "output changed"


@dataclass(frozen=True)
class Migration:
    job: Job
    moves: tuple[tuple[str, str], ...]
    cause: MoveCause
    detail: str = ""
    parts: tuple[Changed, ...] | None = ()
    record: Record | None = None


def claimed(dag: Dag) -> set[str]:
    paths = set(dag.external_inputs)
    for job in dag.jobs:
        paths.update(job.input_paths)
        paths.update(job.output_paths)
    return paths


@dataclass(frozen=True)
class Move:
    old: str
    new: str


@dataclass(frozen=True)
class Blocked:
    cause: MoveCause
    detail: str = ""


def movable(
    dag: Dag, job: Job, record: Record | None, claimed: set[str], moving: set[str], seen: Stamps | None = None
) -> Migration | None:
    if record is None or record.output_paths is None or _in_place(job, record.output_paths):
        return None
    if record.fingerprint != job.work.fingerprint:
        return Migration(job, (), MoveCause.WORK_CHANGED, parts=changed_parts(record, job))
    if _inputs_changed(dag, job, record, moving, seen):
        return Migration(job, (), MoveCause.INPUT_CHANGED)
    moves = []
    for slot, new in job.output_paths_by_slot.items():
        match _move(dag, record, record.output_paths.get(slot), new, slot, claimed):
            case Blocked(cause, detail):
                return Migration(job, (), cause, detail)
            case Move(old, new):
                moves.append((old, new))
    return Migration(job, tuple(moves), MoveCause.MOVABLE, record=record)


def _in_place(job: Job, recorded: dict[str, str]) -> bool:
    return all(recorded.get(slot, path) == path for slot, path in job.output_paths_by_slot.items())


def _inputs_changed(dag: Dag, job: Job, record: Record, moving: set[str], seen: Stamps | None) -> bool:
    staying = {slot: path for slot, path in job.input_paths_by_slot.items() if path not in moving}
    now = stamps(dag, tuple(staying.values()), seen)
    return any(value is None for value in now.values()) or changed(
        compare_files(dag, staying, now, record.inputs, record.input_hashes, Compare.CONTENT, {})
    )


def _move(dag: Dag, record: Record, old: str | None, new: str, slot: str, claimed: set[str]) -> Move | Blocked:
    if old is None:
        return Blocked(MoveCause.PATH_UNRECORDED)
    if old == new:
        return Blocked(MoveCause.PARTLY_IN_PLACE)
    try:
        old_path, new_path = dag.checked(old), dag.checked(new)
    except DagError as exc:
        return Blocked(MoveCause.BAD_PATH, str(exc))
    if os.path.lexists(new_path):
        return Blocked(MoveCause.DESTINATION_EXISTS, new)
    if old in claimed:
        return Blocked(MoveCause.STILL_USED, old)
    if stored(stamp(old_path, new in dag.folders)) != record.outputs.get(slot):
        return Blocked(MoveCause.OUTPUT_CHANGED, old)
    return Move(old, new)
