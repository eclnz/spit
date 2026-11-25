from __future__ import annotations

import tempfile
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ListRead:
    paths: tuple[str, ...]

    def __str__(self) -> str:
        return f"@list({', '.join(self.paths)})"


@contextmanager
def written(reads: tuple[ListRead, ...], path: Callable[[str], Path]) -> Iterator[dict[ListRead, str] | str]:
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
