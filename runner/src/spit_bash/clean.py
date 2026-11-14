"""The files and folders under a folder the user names that the DAG does not explain, and their removal."""

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

# What `Layout.edited` names an edited output kept beside the one that replaced it.
EDITED = re.compile(r"^(?P<name>.+)\.edited-[^/]+$")


@dataclass(frozen=True)
class Stray:
    """A file, link or folder under the cleaned folder that no path of the DAG explains."""

    path: Path
    size: int
    folder: bool
    # The (dimension, value) pairs its path names that no artifact of the DAG has, sorted.
    unknown: tuple[tuple[str, str], ...]
    # Why it cannot be deleted, or None when it can.
    blocked: str | None


def _absolute(dag: Dag, relative: str) -> Path:
    return dag.base(relative).joinpath(*PurePosixPath(relative).parts)


def _refuse(folder: Path, dag: Dag) -> None:
    """Raise when `folder` is not the pipeline's to clean, as the README's `clean` section lists."""
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
    """Each path under `folder` that is neither a path of the DAG, nor inside an output folder, nor in `kept`,
    nor an edited output the runner kept beside its replacement; a folder with nothing explained in it is one
    stray. Raises `DagError` when `folder` is not the pipeline's to clean.

    `folder` is resolved; `kept` names the runner's folder and the files that describe the pipeline.
    """
    _refuse(folder, dag)
    explained = {_absolute(dag, path) for job in dag.jobs for path in (*job.input_paths, *job.output_paths)}
    explained.update(kept)
    owned = {_absolute(dag, path) for path in dag.folders}
    holding = {parent for path in explained for parent in path.parents}
    known = {pair for job in dag.jobs for artifact in (*job.input_artifacts, *job.output_artifacts)
             for pair in artifact.entities}
    patterns = {dimension: re.compile(rf"(?:^|[/_.-]){re.escape(dimension)}[=-]([A-Za-z0-9]+)")
                for dimension, _ in known}
    found = []
    # A worklist rather than recursion, since the folder's depth is the user's.
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
    """Why nothing in `directory` can be deleted, or None when it can be."""
    return None if os.access(directory, os.W_OK | os.X_OK) else "its folder is not writable"


def remove(dag: Dag, found: tuple[Stray, ...], records: dict[str, Record]) -> tuple[list[str], tuple[StateWrite, ...]]:
    """Delete each stray that can be, and give why each other was not, and the writes that forget each record
    of a job the DAG does not have whose outputs a deleted stray held and are now all gone."""
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
    """The strays, grouped by the entities their paths name that the DAG has none of, each with its size.

    Paths are shown from `shown`, the root the folder is under.
    """
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
