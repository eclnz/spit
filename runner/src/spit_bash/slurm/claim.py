from __future__ import annotations

import os
from pathlib import Path


def folder(slurm: Path) -> Path:
    return slurm / "claims"


def name(intent: str, index: int) -> str:
    return f"{intent}-{index}"


def take(slurm: Path, intent: str, index: int, task: str) -> bool:
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
