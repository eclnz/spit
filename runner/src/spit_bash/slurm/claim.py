"""Claims: which task may run an intent's task at an index. One at a time, whatever Slurm does.

A node takes its task's claim before it touches an output, with an exclusive create the file server decides, so of an
intent's task and its twin, which an sbatch that never answered and was made again can leave, one runs and the other
touches nothing. A claim is named by its intent and index, and an intent by its submission, so no claim's path is ever
used twice: a node whose cache still shows an earlier claim never mistakes it for this one's. The controller deletes a
claim once no live task holds it, to keep the folder small.
"""

from __future__ import annotations

import os
from pathlib import Path


def folder(slurm: Path) -> Path:
    return slurm / "claims"


def name(intent: str, index: int) -> str:
    """The claim's file name for the task at `index` of `intent`."""
    return f"{intent}-{index}"


def take(slurm: Path, intent: str, index: int, task: str) -> bool:
    """Take the claim of `intent`'s task at `index` for `task`, as `123_4`; whether this task holds it.

    A failed create is read rather than trusted: some filesystems report a create that lost the race as an error other
    than "exists". The claim is this task's only if the file names it.
    """
    path = folder(slurm) / name(intent, index)
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
    except OSError:
        try:
            return path.read_text(encoding="utf-8").strip() == task
        except OSError:
            return False
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        stream.write(task)
        stream.flush()
        os.fsync(stream.fileno())
    return True


def holders(slurm: Path) -> dict[str, str]:
    """Each claim, by file name, with the task holding it."""
    found = {}
    try:
        entries = list(os.scandir(folder(slurm)))
    except OSError:
        return found
    for entry in entries:
        try:
            found[entry.name] = Path(entry.path).read_text(encoding="utf-8").strip()
        except OSError:
            continue
    return found


def release(slurm: Path, claim: str) -> None:
    (folder(slurm) / claim).unlink(missing_ok=True)
