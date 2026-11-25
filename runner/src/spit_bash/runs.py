from __future__ import annotations

import json
import os
import shutil
import sys
import time
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import IO, Iterable

from .model import DagError
from .state.record import Record
from .state.store import Store

STAMP = "%Y-%m-%dT%H-%M-%S"


def stamp() -> str:
    return time.strftime(STAMP)


def log_name(key: str) -> str:
    made = json.loads(key)
    if not made:
        return "_.log"
    product, entities = made[0]
    if not entities:
        return f"{product}.log"
    pairs = entities.items() if isinstance(entities, dict) else entities
    return f"{product}/{'_'.join(f'{name}={value}' for name, value in pairs)}.log"


@dataclass(frozen=True)
class Run:
    folder: Path

    @property
    def name(self) -> str:
        return self.folder.name

    @property
    def log(self) -> Path:
        return self.folder / "run.log"

    @property
    def plan(self) -> Path:
        return self.folder / "plan.json"

    @property
    def summary(self) -> Path:
        return self.folder / "summary.json"

    def job_log(self, key: str) -> Path:
        return self.folder / "jobs" / log_name(key)

    def replaced(self, key: str) -> Path:
        return self.folder / "replaced" / log_name(key).removesuffix(".log")


def start(runs: Path, started: str) -> Run:
    runs.mkdir(parents=True, exist_ok=True)
    name, number = started, 1
    while True:
        try:
            (runs / name).mkdir()
            return Run(runs / name)
        except FileExistsError:
            number += 1
            name = f"{started}-{number}"


def history(runs: Path) -> list[Run]:
    try:
        names = [entry.name for entry in os.scandir(runs) if entry.is_dir()]
    except FileNotFoundError:
        return []
    width = len(time.strftime(STAMP, time.gmtime(0)))

    def order(name: str) -> tuple[str, int]:
        number = name[width + 1 :]
        return name[:width], int(number) if number.isdigit() else 1

    return [Run(runs / name) for name in sorted(names, key=order, reverse=True)]


class Tee:
    def __init__(self, terminal: IO[str], log: IO[str]):
        self.terminal = terminal
        self.log = log

    def write(self, text: str) -> int:
        self.terminal.write(text)
        self.log.write(text)
        self.log.flush()
        return len(text)

    def flush(self) -> None:
        self.terminal.flush()
        self.log.flush()

    def isatty(self) -> bool:
        return self.terminal.isatty()

    def fileno(self) -> int:
        return self.terminal.fileno()

    @property
    def encoding(self) -> str:
        return self.terminal.encoding


class Recording:
    def __init__(self, run: Run):
        self.run = run
        self.stream: IO[str] | None = None

    def __enter__(self) -> Recording:
        self.stream = self.run.log.open("a", encoding="utf-8")
        self.saved = sys.stdout, sys.stderr
        sys.stdout = Tee(sys.stdout, self.stream)
        sys.stderr = Tee(sys.stderr, self.stream)
        return self

    def __exit__(self, *exc: object) -> None:
        sys.stdout, sys.stderr = self.saved
        if self.stream is not None:
            self.stream.close()


class Ending(str, Enum):
    FINISHED = "finished"
    BLOCKED = "blocked"
    OVER_MEMORY = "over memory"
    DECLINED = "declined"
    INTERRUPTED = "interrupted"
    TERMINATED = "terminated"
    SETUP_FAILED = "Slurm setup or reconciliation failed"
    RESTORED = "restored"
    RELEASED = "released"


def finish(run: Run, ended: Ending, counts: dict[str, object]) -> None:
    run.summary.write_text(json.dumps({"ended": ended.value, **counts}, indent=2) + "\n", encoding="utf-8")


@dataclass(frozen=True)
class Kept:
    key: str
    record: Record | None
    files: tuple[tuple[str, Path, Path], ...]


RECORD = "record.json"


def keep(run: Run, key: str, record: Record | None, files: Iterable[tuple[str, Path]]) -> Path:
    folder = run.replaced(key)
    folder.mkdir(parents=True, exist_ok=True)
    document = {
        "key": key,
        "record": None if record is None else json.loads(record.model_dump_json()),
        "files": [{"path": path, "at": str(at)} for path, at in files],
    }
    (folder / RECORD).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    return folder


def kept_path(run: Run, key: str, path: str) -> Path:
    return run.replaced(key) / "files" / path


def kept(run: Run) -> list[Kept]:
    found = []
    root = run.folder / "replaced"
    for document in sorted(root.rglob(RECORD)) if root.is_dir() else ():
        try:
            data = json.loads(document.read_text(encoding="utf-8"))
            record = None if data["record"] is None else Record.model_validate(data["record"])
            files = tuple(
                (item["path"], Path(item["at"]), document.parent / "files" / item["path"]) for item in data["files"]
            )
        except (OSError, ValueError, KeyError, TypeError) as exc:
            raise DagError(f"cannot read {document}: {exc}") from exc
        found.append(Kept(data["key"], record, files))
    return sorted(found, key=lambda each: each.key)


def size(paths: Iterable[Path]) -> int:
    total = 0
    for path in paths:
        if path.is_file():
            total += path.stat().st_size
        elif path.is_dir():
            total += sum(each.stat().st_size for each in path.rglob("*") if each.is_file())
    return total


def prune(runs: Path, keep_runs: int, records: dict[str, Record]) -> tuple[int, int]:
    removed = kept_logs = 0
    named: dict[str, set[str]] = {}
    for key, record in records.items():
        if record.run is not None:
            named.setdefault(record.run, set()).add(key)
    for run in history(runs)[keep_runs:]:
        keys = named.get(run.name, set())
        logs = {run.job_log(key) for key in keys if run.job_log(key).exists()}
        if not logs:
            shutil.rmtree(run.folder)
            removed += 1
            continue
        for entry in run.folder.rglob("*"):
            if entry.is_file() and entry not in logs:
                entry.unlink()
        for entry in sorted((each for each in run.folder.rglob("*") if each.is_dir()), reverse=True):
            if not any(entry.iterdir()):
                entry.rmdir()
        kept_logs += len(logs)
        removed += 1
    return removed, kept_logs


@dataclass(frozen=True)
class Restored:
    key: str
    paths: tuple[str, ...]
    kept: tuple[str, ...]


def restore(source: Run, undo: Run, state: Store, keys: set[str] | None) -> list[Restored]:
    restored = []
    for each in kept(source):
        if keys is not None and each.key not in keys:
            continue
        aside = []
        for path, at, _ in each.files:
            if at.is_symlink() or at.exists():
                destination = kept_path(undo, each.key, path)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(at, destination)
                aside.append((path, at))
        if aside:
            keep(undo, each.key, state.jobs.get(each.key), aside)
        for path, at, stored_at in each.files:
            at.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(stored_at, at)
        if each.record is None:
            state.forget(each.key)
        else:
            state.record(each.key, each.record)
        shutil.rmtree(source.replaced(each.key))
        restored.append(Restored(each.key, tuple(path for path, _, _ in each.files), tuple(path for path, _ in aside)))
    return restored
