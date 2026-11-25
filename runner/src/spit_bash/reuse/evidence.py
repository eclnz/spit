from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from ..files import Stamp, Stamps, content_hash, stored
from ..model import Dag, Job
from ..state.record import Record
from ..cause import TOOL, Changed, Part, Tool, part_key


class Compare(Enum):
    STAT = "stat"
    CONTENT = "content"
    FULL = "full"


class Content(Enum):
    SAME = "same"
    DIFFERS = "differs"
    NO_HASH = "no hash"
    NOT_COMPARED = "not compared"
    UNREADABLE = "unreadable"


@dataclass(frozen=True)
class FileDiff:
    slot: str
    path: str
    recorded: Stamp | None
    now: Stamp | None
    content: Content

    @property
    def changed(self) -> bool:
        return self.content is not Content.SAME


@dataclass(frozen=True)
class Evidence:
    job: Job
    record: Record | None
    now: Stamps
    work_changed: bool = False
    parts: tuple[Changed, ...] | None = ()
    checks_changed: bool = False
    inputs: tuple[FileDiff, ...] = ()
    outputs: tuple[FileDiff, ...] = ()


def changed_parts(record: Record, job: Job) -> tuple[Changed, ...] | None:
    if record.parts is None:
        return None
    named = (*record.parts, *job.work.parts)
    tools = sorted({Tool(key.removeprefix(TOOL)) for key in named if key.startswith(TOOL)})
    every: tuple[Changed, ...] = (*Part, *tools)
    return tuple(part for part in every if record.parts.get(part_key(part)) != job.work.parts.get(part_key(part)))


def compare_files(
    dag: Dag,
    paths: dict[str, str],
    now: Stamps,
    recorded: dict[str, list[int]],
    recorded_hashes: dict[str, str] | None,
    compare: Compare,
    hashes: dict[str, str | None],
    every: bool = False,
) -> tuple[FileDiff, ...]:
    diffs = []
    found_changed = False
    for slot, path in paths.items():
        was, current = recorded.get(slot), now.get(path)
        if compare is not Compare.FULL and stored(current) == was:
            continue
        if compare is Compare.STAT or (found_changed and not every):
            content = Content.NOT_COMPARED
        elif not recorded_hashes or slot not in recorded_hashes:
            content = Content.NO_HASH
        elif path not in hashes and current is None:
            content = Content.UNREADABLE
        else:
            if path not in hashes:
                hashes[path] = content_hash(str(dag.path(path)), path in dag.folders)
            held = hashes[path]
            content = (
                Content.UNREADABLE
                if held is None
                else Content.SAME
                if held == recorded_hashes[slot]
                else Content.DIFFERS
            )
        diffs.append(FileDiff(slot, path, None if was is None else Stamp.from_stored(was), current, content))
        found_changed = found_changed or content is not Content.SAME
    return tuple(diffs)


def changed(diffs: tuple[FileDiff, ...]) -> bool:
    return any(diff.changed for diff in diffs)


def evidence(dag: Dag, job: Job, record: Record | None, now: Stamps, compare: Compare, every: bool = False) -> Evidence:
    if record is None:
        return Evidence(job, None, now)
    work_changed = record.fingerprint != job.work.fingerprint
    parts = changed_parts(record, job) if work_changed else ()
    found = Evidence(job, record, now, work_changed, parts, record.checks != job.work.checks_key)
    if work_changed and not every:
        return found
    hashes: dict[str, str | None] = {}
    inputs = compare_files(
        dag, job.input_paths_by_slot, now, record.inputs, record.input_hashes, compare, hashes, every
    )
    if changed(inputs) and not every:
        return Evidence(job, record, now, work_changed, parts, found.checks_changed, inputs)
    outputs = compare_files(
        dag, job.output_paths_by_slot, now, record.outputs, record.output_hashes, compare, hashes, every
    )
    return Evidence(job, record, now, work_changed, parts, found.checks_changed, inputs, outputs)
