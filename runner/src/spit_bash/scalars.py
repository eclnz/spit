from __future__ import annotations

import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Mapping

NUMBER = re.compile(r"[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?")

LIMIT = 4096
SHOWN = 40


def _bound(value: float) -> str:
    text = repr(value)
    return text.removesuffix(".0")


@dataclass(frozen=True)
class Read:
    path: str
    least: float | None = None
    greatest: float | None = None

    def __str__(self) -> str:
        bounds = "".join(f", {key}={_bound(value)}" for key, value in (("min", self.least), ("max", self.greatest))
                         if value is not None)
        return f"@scalar({self.path}{bounds})"

    def fault(self, text: str) -> str | None:
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
    pieces: tuple[str | Read | object, ...]

    def __new__(cls, pieces: tuple[str | Read | object, ...]) -> Reading:
        argument = super().__new__(cls, "".join(map(str, pieces)))
        argument.pieces = pieces
        return argument


def reads(commands: Iterable[tuple[str, ...]]) -> tuple[Read, ...]:
    found: dict[Read, None] = {}
    for command in commands:
        for argument in command:
            if isinstance(argument, Reading):
                found.update((piece, None) for piece in argument.pieces if isinstance(piece, Read))
    return tuple(found)


def bound(command: tuple[str, ...], values: dict[str, str], files: Mapping[object, str] | None = None) -> tuple[str, ...]:
    files = files or {}

    def text(piece: str | Read | object) -> str:
        if isinstance(piece, str):
            return piece
        return values[piece.path] if isinstance(piece, Read) else files[piece]

    return tuple("".join(map(text, argument.pieces)) if isinstance(argument, Reading) else argument
                 for argument in command)


def _shown(text: str) -> str:
    if not text:
        return "nothing"
    if len(text) > SHOWN or any(not character.isprintable() or character.isspace() for character in text):
        return repr(text[:SHOWN] + ("..." if len(text) > SHOWN else ""))
    return text


def read_all(found: tuple[Read, ...], path: Callable[[str], Path]) -> dict[str, str] | tuple[str, str]:
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
