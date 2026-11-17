"""An intent: one array as the controller means to submit it, written before sbatch, named by what it holds.

An intent's name is a hash of its manifest, the identity of each task by array index, sorted by key, of the
resources the array asks for, and of the submission. The script Slurm runs is the intent's own, so the queue shows
each task's intent with its index, and the manifest says what the task does. An sbatch that never answered is made
again with the same intent, so a twin it leaves shares its tasks' claims.
"""

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
    # Each task's identity, by array index.
    manifest: tuple[Identity, ...]


def name(manifest: tuple[Identity, ...], resources: tuple, submission: str) -> str:
    """The intent's name: a hash of its manifest, its resources, and the submission that makes it, as the run and a
    count, so that no two submissions share a name, and so no claim's path is used twice."""
    text = json.dumps([[[each.key, each.fingerprint, each.mode.value] for each in manifest], list(resources),
                       submission])
    return hashlib.sha256(text.encode()).hexdigest()[:20]


def folder(slurm: Path) -> Path:
    return slurm / "intents"


def script(slurm: Path, intent: str) -> Path:
    """The intent's script, which Slurm runs, and squeue shows for each task."""
    return folder(slurm) / f"{intent}.sh"


def write(slurm: Path, intent: Intent, work: Batch, setup: tuple[str, ...], command: tuple[str, ...]) -> Path:
    """Write the intent's manifest, batch and script, each whole or not at all, before sbatch; its script."""
    place = folder(slurm)
    place.mkdir(parents=True, exist_ok=True)
    _atomic(place / f"{intent.name}.json", json.dumps(
        [[each.key, each.fingerprint, each.mode.value] for each in intent.manifest]).encode())
    batches.write(work, place / f"{intent.name}.pickle")
    run = shlex.join([*command, "job", str(place / f"{intent.name}.pickle")])
    path = script(slurm, intent.name)
    _atomic(path, ("#!/bin/bash\n" + "".join(f"{line}\n" for line in setup)
                   + f'exec {run} "$SLURM_ARRAY_TASK_ID"\n').encode())
    return path


def _atomic(path: Path, data: bytes) -> None:
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


class Manifests:
    """The intents of a Slurm folder, each read once, which identify the tasks the queue shows."""

    def __init__(self, slurm: Path) -> None:
        self.slurm = slurm
        self._read: dict[str, tuple[Identity, ...] | None] = {}

    def intent_of(self, command: str) -> str | None:
        """The intent a task's script names, from the command squeue shows, or None for any other script."""
        path = Path(command.split()[0]) if command.split() else Path()
        if path.parent != folder(self.slurm) or path.suffix != ".sh":
            return None
        return path.stem

    def identify(self, task: Task) -> Identity | None:
        """What the task does, or None when its intent is gone or does not reach its index."""
        if task.intent is None:
            return None
        if task.intent not in self._read:
            try:
                rows = json.loads((folder(self.slurm) / f"{task.intent}.json").read_text(encoding="utf-8"))
                self._read[task.intent] = tuple(Identity(key, fingerprint, Mode(mode))
                                                for key, fingerprint, mode in rows)
            except (OSError, ValueError, TypeError):
                self._read[task.intent] = None
        manifest = self._read[task.intent]
        return None if manifest is None or task.index >= len(manifest) else manifest[task.index]

    def remove_unused(self, used: set[str]) -> None:
        """Delete every intent no task of the queue belongs to, once no controller can be submitting one."""
        try:
            entries = list(os.scandir(folder(self.slurm)))
        except OSError:
            return
        for entry in entries:
            stem = entry.name.split(".")[0]
            if stem and stem not in used:
                Path(entry.path).unlink(missing_ok=True)
                self._read.pop(stem, None)
