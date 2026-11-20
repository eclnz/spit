"""A job's outcome as JSON, and back: the failures, stamps and command ends that a result file carries.

Each dataclass, named tuple, enum and path is written with its type's name under `$`, and read back only as one of
the types in `TYPES`, so a result file can name no other class.
"""

from __future__ import annotations

import dataclasses
from enum import Enum
from pathlib import Path
from typing import Any

from ..execute import failure, process
from ..execute.job import Made
from ..files import Stamp
from ..model import Check, Side

TYPES: dict[str, type] = {kind.__name__: kind for kind in (
    Made, Stamp, Check, Side, failure.Phase,
    failure.InputDisappeared, failure.NoCommand, failure.CheckArtifactMissing, failure.CheckFailed,
    failure.CommandFailed, failure.ScalarInvalid, failure.CalculationFailed, failure.ListNotWritten, failure.SeedNotCopied,
    failure.StdoutNotMoved, failure.InputChanged, failure.InputNotVisible, failure.OutputsMissing, failure.PlaceFailed,
    process.Exited, process.KilledMemory, process.KilledSignal, process.NotStarted,
)}


def encode(value: Any) -> Any:
    """`value` as JSON values: lists for tuples, and a `$` object for each typed value."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, Enum):
        return {"$": type(value).__name__, "value": value.value}
    if isinstance(value, Path):
        return {"$": "Path", "value": str(value)}
    if isinstance(value, tuple) and hasattr(value, "_fields"):
        return {"$": type(value).__name__, "fields": [encode(item) for item in value]}
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {"$": type(value).__name__,
                **{field.name: encode(getattr(value, field.name)) for field in dataclasses.fields(value)}}
    if isinstance(value, (tuple, list)):
        return [encode(item) for item in value]
    if isinstance(value, dict):
        return {"$": "dict", "items": [[encode(key), encode(item)] for key, item in value.items()]}
    raise TypeError(f"cannot write a {type(value).__name__} to a result file")


def decode(value: Any) -> Any:
    """What `encode` wrote, read back; lists become tuples. Raises `ValueError` for a type it does not know."""
    if isinstance(value, list):
        return tuple(decode(item) for item in value)
    if not isinstance(value, dict):
        return value
    name = value.get("$")
    if name == "Path":
        return Path(value["value"])
    if name == "dict":
        return {decode(key): decode(item) for key, item in value["items"]}
    kind = TYPES.get(name) if isinstance(name, str) else None
    if kind is None:
        raise ValueError(f"a result file names an unknown type {name!r}")
    if issubclass(kind, Enum):
        return kind(value["value"])
    if issubclass(kind, tuple):
        return kind(*(decode(item) for item in value["fields"]))
    return kind(**{key: decode(item) for key, item in value.items() if key != "$"})
