"""The `list` parts of a job's commands: files listing a `many` input's paths, made when the job starts."""

from __future__ import annotations

import tempfile
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from . import scalars


@dataclass(frozen=True)
class ListRead:
    """A `list` part: the input paths its file lists, one to a line, in order."""

    paths: tuple[str, ...]

    def __str__(self) -> str:
        """The part as `spit dag --commands` shows it: `@list(in/2.txt, in/10.txt)`."""
        return f"@list({', '.join(self.paths)})"


def found(commands: tuple[tuple[str, ...], ...]) -> tuple[ListRead, ...]:
    """Each `list` part among the commands' arguments, once each, in the order they come."""
    found: dict[ListRead, None] = {}
    for command in commands:
        for argument in command:
            if isinstance(argument, scalars.Reading):
                found.update((piece, None) for piece in argument.pieces if isinstance(piece, ListRead))
    return tuple(found)


@contextmanager
def written(reads: tuple[ListRead, ...], path: Callable[[str], Path]) -> Iterator[dict[ListRead, str] | str]:
    """Each list's file, by its read, written to a folder of its own that is removed when the job ends,
    whether it succeeds, fails or is cancelled; or, when a file cannot be written, why.

    `path` gives each listed file's absolute path. The files' names are not part of the job's work,
    so a later run that writes them elsewhere is the same work.
    """
    if not reads:
        yield {}
        return
    try:
        folder = tempfile.TemporaryDirectory(prefix="spit-bash-lists-")
    except OSError as exc:
        yield f"cannot make a folder for the job's lists: {exc.strerror or exc}"
        return
    with folder:
        files: dict[ListRead, str] = {}
        try:
            for index, read in enumerate(reads):
                lines = [str(path(member)) for member in read.paths]
                broken = next((line for line in lines if "\n" in line or "\r" in line), None)
                if broken is not None:
                    yield f"cannot list {broken!r}, whose path holds a line break, one path to a line"
                    return
                target = Path(folder.name) / f"list{index}.txt"
                target.write_text("".join(f"{line}\n" for line in lines), encoding="utf-8")
                files[read] = str(target)
        except OSError as exc:
            yield f"cannot write the file for {read}: {exc.strerror or exc}"
            return
        yield files
