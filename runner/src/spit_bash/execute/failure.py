from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Union

from ..files import Stamps
from ..model import Check
from ..plan import Decision
from .process import CommandEnd


class Phase(Enum):
    VERIFY = "verify"
    COMMAND = "command"


@dataclass(frozen=True)
class InputDisappeared:
    pass


@dataclass(frozen=True)
class NoCommand:
    pass


@dataclass(frozen=True)
class CheckArtifactMissing:
    check: Check


@dataclass(frozen=True)
class CheckFailed:
    check: Check
    end: CommandEnd


@dataclass(frozen=True)
class CommandFailed:
    phase: Phase
    index: int
    end: CommandEnd


@dataclass(frozen=True)
class ScalarInvalid:
    path: str
    words: str


@dataclass(frozen=True)
class CalculationFailed:
    words: str


@dataclass(frozen=True)
class ListNotWritten:
    words: str


@dataclass(frozen=True)
class SeedNotCopied:
    path: str
    words: str


@dataclass(frozen=True)
class StdoutNotMoved:
    path: str
    error: str


@dataclass(frozen=True)
class InputChanged:
    before: Stamps
    after: Stamps
    folders: tuple[str, ...]


@dataclass(frozen=True)
class OutputsMissing:
    missing: tuple[tuple[str, str], ...]
    written: tuple[str, ...] = ()


@dataclass(frozen=True)
class PlaceFailed:
    error: str
    put_back: bool


@dataclass(frozen=True)
class InputNotVisible:
    unseen: tuple[tuple[str, tuple[int, ...], tuple[int, ...] | None], ...]
    seconds: float


@dataclass(frozen=True)
class SlurmEnded:
    state: str | None
    words: str
    worker_failure: Failure | None = None


@dataclass(frozen=True)
class DependencyFailed:
    first: int


@dataclass(frozen=True)
class Blocked:
    decision: Decision


@dataclass(frozen=True)
class ScreenFailed:
    screen: str
    path: str
    reason: str


Failure = Union[
    ScreenFailed,
    InputDisappeared,
    NoCommand,
    CheckArtifactMissing,
    CheckFailed,
    CommandFailed,
    ScalarInvalid,
    CalculationFailed,
    ListNotWritten,
    SeedNotCopied,
    StdoutNotMoved,
    InputChanged,
    OutputsMissing,
    PlaceFailed,
    InputNotVisible,
    SlurmEnded,
    DependencyFailed,
    Blocked,
]
