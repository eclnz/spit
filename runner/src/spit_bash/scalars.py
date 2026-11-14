"""The numbers a job's commands read from its input files, as the DAG's `scalar` parts give them."""

from __future__ import annotations

import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Mapping

# One number written as a decimal: an optional sign, digits with at most one `.`, and an optional
# exponent. Keep in step with `decimal` in SPIT's `src/template/command.rs`.
NUMBER = re.compile(r"[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?")

# The most of a file a read looks at: a number is short, and a file longer than this is not one.
LIMIT = 4096
# How much of what a file holds an error shows.
SHOWN = 40


def _bound(value: float) -> str:
    """A bound as SPIT writes it: `5`, not `5.0`."""
    text = repr(value)
    return text.removesuffix(".0")


@dataclass(frozen=True)
class Read:
    """A `scalar` part: the input file that holds the number, and the least and greatest values allowed."""

    path: str
    least: float | None = None
    greatest: float | None = None

    def __str__(self) -> str:
        """The read as `spit dag --commands` shows it: `@scalar(out/tr/sub=01.txt, min=5)`."""
        bounds = "".join(f", {key}={_bound(value)}" for key, value in (("min", self.least), ("max", self.greatest))
                         if value is not None)
        return f"@scalar({self.path}{bounds})"

    def fault(self, text: str) -> str | None:
        """Why `text`, what the file holds without the whitespace around it, is not a number this read
        allows, as words to follow what it holds; None when it is one."""
        if not NUMBER.fullmatch(text):
            return "which is not one number"
        value = float(text)
        if not math.isfinite(value):
            return "which is too large to be a number"
        if self.least is not None and value < self.least:
            return f"which is under the min of {_bound(self.least)}"
        if self.greatest is not None and value > self.greatest:
            return f"which is over the max of {_bound(self.greatest)}"
        return None


class Reading(str):
    """An argument that holds a read, or another piece filled in when the job starts, such as a
    `lists.ListRead`: its text shows each as its `str` writes it, so plans and errors show the
    argument whole, and `pieces` are its text and reads, in order, for `bound` to fill in."""

    pieces: tuple[str | Read | object, ...]

    def __new__(cls, pieces: tuple[str | Read | object, ...]) -> Reading:
        argument = super().__new__(cls, "".join(map(str, pieces)))
        argument.pieces = pieces
        return argument


def reads(commands: Iterable[tuple[str, ...]]) -> tuple[Read, ...]:
    """Each read among the commands' arguments, once each, in the order they come."""
    found: dict[Read, None] = {}
    for command in commands:
        for argument in command:
            if isinstance(argument, Reading):
                found.update((piece, None) for piece in argument.pieces if isinstance(piece, Read))
    return tuple(found)


def bound(command: tuple[str, ...], values: dict[str, str], files: Mapping[object, str] | None = None) -> tuple[str, ...]:
    """The command with each read replaced by the number its file holds, from `values` by path, and each
    other piece filled in when the job starts, a `lists.ListRead`, by its file's path from `files`."""
    files = files or {}

    def text(piece: str | Read | object) -> str:
        if isinstance(piece, str):
            return piece
        return values[piece.path] if isinstance(piece, Read) else files[piece]

    return tuple("".join(map(text, argument.pieces)) if isinstance(argument, Reading) else argument
                 for argument in command)


def _shown(text: str) -> str:
    """What a file holds, as an error shows it."""
    if not text:
        return "nothing"
    if len(text) > SHOWN or any(not character.isprintable() or character.isspace() for character in text):
        return repr(text[:SHOWN] + ("..." if len(text) > SHOWN else ""))
    return text


def read_all(found: tuple[Read, ...], path: Callable[[str], Path]) -> dict[str, str] | tuple[str, str]:
    """The number each read's file holds, by its path, with `path` giving where a file is; or the first
    file that does not hold a number its reads allow, with words saying what it holds and why."""
    values: dict[str, str] = {}
    for read in found:
        text = values.get(read.path)
        if text is None:
            try:
                with open(path(read.path), "rb") as stream:
                    data = stream.read(LIMIT + 1)
            except OSError as exc:
                return read.path, f"could not be read: {exc.strerror or exc}"
            if len(data) > LIMIT:
                return read.path, f"holds more than {LIMIT} bytes, which is not one number"
            try:
                text = data.decode().strip()
            except UnicodeDecodeError:
                return read.path, "holds bytes that are not text, which is not one number"
        fault = read.fault(text)
        if fault is not None:
            return read.path, f"holds {_shown(text)}, {fault}"
        values[read.path] = text
    return values
