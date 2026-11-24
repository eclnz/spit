from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Union

from ..model import Job

if TYPE_CHECKING:
    from .scheduler import Result


@dataclass(frozen=True)
class RunsAlone:
    job: Job
    over: str


@dataclass(frozen=True)
class Running:
    runs: int
    maybe: int
    remaining: float | None
    places: int = 0


@dataclass(frozen=True)
class Queued:
    job: Job
    task: str


@dataclass(frozen=True)
class Retrying:
    job: Job
    attempt: int
    why: str
    resources: str


@dataclass(frozen=True)
class Started:
    job: Job


@dataclass(frozen=True)
class Hashing:
    job: Job


@dataclass(frozen=True)
class Done:
    job: Job
    seconds: float
    progress: str


@dataclass(frozen=True)
class Placed:
    job: Job
    moves: tuple[tuple[str, str], ...]


@dataclass(frozen=True)
class Checking:
    job: Job


@dataclass(frozen=True)
class Checked:
    job: Job


@dataclass(frozen=True)
class SkippedUnchanged:
    job: Job


@dataclass(frozen=True)
class JobFailed:
    result: Result


@dataclass(frozen=True)
class NotRun:
    result: Result


@dataclass(frozen=True)
class ScreenNote:
    text: str


Event = Union[
    ScreenNote,
    RunsAlone,
    Running,
    Queued,
    Retrying,
    Started,
    Hashing,
    Done,
    Placed,
    Checking,
    Checked,
    SkippedUnchanged,
    JobFailed,
    NotRun,
]
