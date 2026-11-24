from __future__ import annotations

import os
import re
import shutil
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from .files import stamp
from .leftovers import amount
from .model import Dag, DagError
from .state.record import Record
from .state.store import StateWrite

EDITED = re.compile(r"^(?P<name>.+)\.edited-[^/]+$")


@dataclass(frozen=True)
class Stray:
    path: Path
    size: int
    folder: bool
    unknown: tuple[tuple[str, str], ...]
    blocked: str | None


def _absolute(dag: Dag, relative: str) -> Path:
    return dag.base(relative).joinpath(*PurePosixPath(relative).parts)


def _refuse(folder: Path, dag: Dag) -> None:
    roots = (dag.root, dag.outputs)
    if not any(folder.is_relative_to(root) for root in roots):
        raise DagError(f"{folder} is under neither the dataset root {dag.root} nor the output root {dag.outputs}")
    if dag.left_out:
        raise DagError("cannot clean with a partial DAG: the outputs of the jobs it leaves out would look like strays")
    for source in dag.external_inputs:
        path = _absolute(dag, source)
        if path.is_relative_to(folder) or folder.is_relative_to(path):
            raise DagError(f"{folder} holds the source {source}, so it is not a folder of outputs only; "
                           "name a folder the pipeline writes to")
    if not any(_absolute(dag, path).is_relative_to(folder) for job in dag.jobs for path in job.output_paths):
        raise DagError(f"no job writes under {folder}, so it is not the pipeline's to clean")


def strays(dag: Dag, folder: Path, kept: tuple[Path, ...]) -> tuple[Stray, ...]:
    _refuse(folder, dag)
    explained = {_absolute(dag, path) for job in dag.jobs for path in (*job.input_paths, *job.output_paths)}
    explained.update(kept)
    explained.update(Path(os.path.abspath(path)) for path in dag.verdict_files)
    explained.update(Path(os.path.realpath(path)) for path in explained.copy())
    owned = {_absolute(dag, path) for path in dag.folders}
    holding = {parent for path in explained for parent in path.parents}
    known = {pair for job in dag.jobs for artifact in (*job.input_artifacts, *job.output_artifacts)
             for pair in artifact.entities}
    patterns = {dimension: re.compile(rf"(?:^|[/_.-]){re.escape(dimension)}[=-]([A-Za-z0-9]+)")
                for dimension, _ in known}
    found = []
    pending = [folder]
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                edited = EDITED.match(entry.name)
                if path in explained or (edited and path.with_name(edited["name"]) in explained):
                    continue
                is_folder = entry.is_dir(follow_symlinks=False)
                if path in holding and is_folder and path not in owned:
                    pending.append(path)
                    continue
                relative = path.relative_to(folder).as_posix()
                unknown = sorted({(dimension, match) for dimension, pattern in patterns.items()
                                  for match in pattern.findall(relative)} - known)
                found.append(Stray(path, _size(entry, is_folder), is_folder, tuple(unknown), _blocked(directory)))
    return tuple(sorted(found, key=lambda stray: (stray.unknown, stray.path)))


def _size(entry: os.DirEntry[str], folder: bool) -> int:
    if folder:
        now = stamp(Path(entry.path), True)
        return 0 if now is None else now.size
    return entry.stat(follow_symlinks=False).st_size


def _blocked(directory: str) -> str | None:
    return None if os.access(directory, os.W_OK | os.X_OK) else "its folder is not writable"


def remove(dag: Dag, found: tuple[Stray, ...], records: dict[str, Record]) -> tuple[list[str], tuple[StateWrite, ...]]:
    failures = []
    deleted = []
    for stray in found:
        try:
            if stray.folder:
                shutil.rmtree(stray.path)
            else:
                stray.path.unlink()
        except OSError as exc:
            failures.append(f"{stray.path}: {exc.strerror}")
            continue
        deleted.append(stray.path)
    claimed = {job.work.key for job in dag.jobs}
    forgotten = []
    for key, record in records.items():
        if key in claimed or not record.output_paths:
            continue
        paths = [_absolute(dag, path) for path in record.output_paths.values()]
        held = any(path == gone or path.is_relative_to(gone) for path in paths for gone in deleted)
        if held and not any(os.path.lexists(path) for path in paths):
            forgotten.append(StateWrite(key, None))
    return failures, tuple(forgotten)


def lines(found: tuple[Stray, ...], folder: Path, shown: Path, deleting: bool) -> list[str]:
    if not found:
        return [f"everything under {folder.relative_to(shown).as_posix() or '.'} is explained by the DAG"]
    total = amount(sum(stray.size for stray in found))
    noun = "path" if len(found) == 1 else "paths"
    where = folder.relative_to(shown).as_posix() or "."
    head = f"{len(found)} {noun} ({total}) under {where} that the DAG does not explain:"
    out = [f"deleting {head}" if deleting else head]
    group: tuple[tuple[str, str], ...] | None = None
    for stray in found:
        indent = "  "
        if stray.unknown:
            indent = "    "
            if stray.unknown != group:
                named = ", ".join(f"{dimension}={value}" for dimension, value in stray.unknown)
                out.append(f"  {named}, which no job of the DAG reads or makes:")
                group = stray.unknown
        size = amount(stray.size)
        why = f", cannot be deleted: {stray.blocked}" if stray.blocked else ""
        out.append(f"{indent}{stray.path.relative_to(shown).as_posix()}{'/' if stray.folder else ''} ({size}{why})")
    if not deleting:
        out.append("run again with --yes to delete them")
    return out
