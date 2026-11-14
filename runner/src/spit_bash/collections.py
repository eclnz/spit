"""Render collection arguments and own the temporary files their jobs use."""

from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass, replace
from pathlib import Path
from tempfile import TemporaryDirectory

from . import scalars


@dataclass(frozen=True)
class ListRead:
    """Input paths whose absolute names are written one per line before verify."""

    paths: tuple[str, ...]

    def __str__(self):
        return f"@list({', '.join(self.paths)})"


@contextmanager
def prepared(job, path):
    """Supply one temporary file per distinct list, shared by verify and commands.

    Files survive every command and are removed on success, failure or cancellation.
    Their names never enter the work fingerprint.
    """
    commands = (*(run.arguments for run in job.commands), *job.verify)
    found = dict.fromkeys(piece for command in commands for argument in command
                          if isinstance(argument, scalars.Reading) for piece in argument.pieces
                          if isinstance(piece, ListRead))
    if not found:
        yield job
        return
    with TemporaryDirectory(prefix="spit-lists-") as folder:
        names = {}
        for index, read in enumerate(found):
            lines = [str(path(p)) for p in read.paths]
            if any("\n" in p or "\r" in p for p in lines):
                raise ValueError("a list-file path contains a newline")
            target = Path(folder) / f"{index}.txt"
            target.write_text("".join(p + "\n" for p in lines), encoding="utf-8")
            names[read] = str(target)

        def bind(command):
            return tuple(scalars.Reading(tuple(names.get(piece, piece) for piece in argument.pieces))
                         if isinstance(argument, scalars.Reading) else argument for argument in command)

        yield replace(job, commands=tuple(replace(run, arguments=bind(run.arguments)) for run in job.commands),
                      verify=tuple(bind(command) for command in job.verify))
