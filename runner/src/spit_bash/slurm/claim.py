"""Claims: which task may run a job's commands. One task at a time per key, whatever Slurm or the controllers do.

A node takes its job's claim before it touches an output, with an exclusive create the file server decides, so of two
tasks of one key that run together, as a twin of a late sbatch can, one runs and the other touches nothing. Only the
controller releases a claim, once no live task holds it.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path


def folder(slurm: Path) -> Path:
    return slurm / "claims"


def name(key: str) -> str:
    """The claim's file name for a key."""
    return hashlib.sha256(key.encode()).hexdigest()[:20]


def take(slurm: Path, key: str, task: str) -> bool:
    """Take the key's claim for `task`, as `123_4`; whether this task holds it.

    A failed create is read rather than trusted: some filesystems report a create that lost the race as an error other
    than "exists". The claim is this task's only if the file names it.
    """
    path = folder(slurm) / name(key)
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
