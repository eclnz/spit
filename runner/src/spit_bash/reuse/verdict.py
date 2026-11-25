from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from ..cause import Cause, Why
from ..state.record import Record
from . import records
from .evidence import Evidence, changed


class Standing(Enum):
    CURRENT = "current"
    CHECKS_ONLY = "checks only"
    STALE = "stale"


@dataclass(frozen=True)
class Verdict:
    standing: Standing
    cause: Cause | None = None
    refreshed: Record | None = None


def verdict(found: Evidence) -> Verdict:
    record = found.record
    if record is None:
        return Verdict(Standing.STALE, Cause(Why.NO_RECORD))
    if found.work_changed:
        return Verdict(Standing.STALE, Cause(Why.WORK_CHANGED, parts=found.parts))
    if changed(found.inputs):
        return Verdict(Standing.STALE, Cause(Why.INPUT_CHANGED))
    if changed(found.outputs):
        return Verdict(Standing.STALE, Cause(Why.OUTPUT_CHANGED))
    refreshed = records.refreshed(record, found.job, found.now)
    return Verdict(Standing.CHECKS_ONLY if found.checks_changed else Standing.CURRENT, None, refreshed)
