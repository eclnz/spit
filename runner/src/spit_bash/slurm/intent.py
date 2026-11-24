from __future__ import annotations

import hashlib
import json
import os
import shlex
import tempfile
from dataclasses import dataclass
from pathlib import Path

from . import batch as batches
from .batch import Batch
from .reconcile import Identity, Mode, Task


@dataclass(frozen=True)
class Intent:
    name: str
    manifest: tuple[Identity, ...]


def name(manifest: tuple[Identity, ...], resources: tuple, submission: str) -> str:
    text = json.dumps(
        [[[each.key, each.fingerprint, each.mode.value] for each in manifest], list(resources), submission]
    )
    return hashlib.sha256(text.encode()).hexdigest()[:20]


def folder(slurm: Path) -> Path:
    return slurm / "intents"


def script(slurm: Path, intent: str) -> Path:
    return folder(slurm) / f"{intent}.sh"


def write(slurm: Path, intent: Intent, work: Batch, setup: tuple[str, ...], command: tuple[str, ...]) -> Path:
    place = folder(slurm)
    place.mkdir(parents=True, exist_ok=True)
    atomic(
        place / f"{intent.name}.json",
        json.dumps([[each.key, each.fingerprint, each.mode.value] for each in intent.manifest]).encode(),
    )
    batches.write(work, place / f"{intent.name}.pickle")
    run = shlex.join([*command, "job", str(place / f"{intent.name}.pickle")])
    path = script(slurm, intent.name)
    atomic(
        path,
        ("#!/bin/bash\n" + "".join(f"{line}\n" for line in setup) + f'exec {run} "$SLURM_ARRAY_TASK_ID"\n').encode(),
    )
    return path


def atomic(path: Path, data: bytes) -> None:
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def remove(slurm: Path, name: str) -> None:
    for suffix in ("json", "pickle", "sh"):
        (folder(slurm) / f"{name}.{suffix}").unlink(missing_ok=True)


class Manifests:
    def __init__(self, slurm: Path) -> None:
        self.slurm = slurm
        self._read: dict[str, tuple[Identity, ...] | None] = {}

    def intent_of(self, command: str) -> str | None:
        path = Path(command)
        if path.parent != folder(self.slurm) or path.suffix != ".sh":
            return None
        return path.stem

    def identify(self, task: Task) -> Identity | None:
        if task.intent is None:
            return None
        if task.intent not in self._read:
            try:
                rows = json.loads((folder(self.slurm) / f"{task.intent}.json").read_text(encoding="utf-8"))
                self._read[task.intent] = tuple(
                    Identity(key, fingerprint, Mode(mode)) for key, fingerprint, mode in rows
                )
            except (OSError, ValueError, TypeError):
                return None
        manifest = self._read[task.intent]
        return None if manifest is None or not 0 <= task.index < len(manifest) else manifest[task.index]

    def remove_unused(self, used: set[str]) -> None:
        try:
            entries = list(os.scandir(folder(self.slurm)))
        except OSError:
            return
        for entry in entries:
            stem = entry.name.split(".")[0]
            if stem and stem not in used:
                Path(entry.path).unlink(missing_ok=True)
                self._read.pop(stem, None)
