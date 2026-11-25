from __future__ import annotations

import hashlib
import os
import stat
from pathlib import Path
from typing import Any, NamedTuple

from .model import Dag, Job

try:
    import xxhash
except ImportError:
    NAME = "blake2b"

    def _new() -> Any:
        return hashlib.blake2b(digest_size=16)
else:
    NAME = "xxh3_128"
    _new = xxhash.xxh3_128

CHUNK = 1 << 20

_FILE_FIELDS = ("size", "mtime_ns")
_FOLDER_FIELDS = ("total size", "latest mtime_ns", "file count")


class Stamp(NamedTuple):
    size: int
    mtime_ns: int
    count: int | None = None

    @classmethod
    def from_stored(cls, value: list[int]) -> Stamp:
        return cls(*value)

    def describe(self) -> str:
        if self.count is None:
            return f"{self.size} bytes, mtime_ns {self.mtime_ns}"
        return f"{self.size} bytes in {self.count} files, latest mtime_ns {self.mtime_ns}"

    def changes(self, now: Stamp) -> list[str]:
        fields = _FILE_FIELDS if self.count is None else _FOLDER_FIELDS
        return [f"{field} {old} -> {new}" for field, old, new in zip(fields, self, now) if old != new]


Stamps = dict[str, Stamp | None]


def stored(stamp: Stamp | None) -> list[int] | None:
    if stamp is None:
        return None
    return [stamp.size, stamp.mtime_ns] if stamp.count is None else [stamp.size, stamp.mtime_ns, stamp.count]


def stored_each(by_key: dict[str, Stamp | None]) -> dict[str, list[int] | None]:
    return {key: stored(value) for key, value in by_key.items()}


def stamp(path: Path, folder: bool = False) -> Stamp | None:
    if folder:
        return _folder_stamp(path)
    try:
        info = path.stat()
    except FileNotFoundError:
        return None
    return Stamp(info.st_size, info.st_mtime_ns) if stat.S_ISREG(info.st_mode) else None


def _folder_stamp(path: Path) -> Stamp | None:
    if not path.is_dir():
        return None
    size, latest, count = 0, 0, 0
    for directory, folders, files in os.walk(path):
        for name in files:
            try:
                info = os.stat(os.path.join(directory, name))
            except FileNotFoundError:
                continue
            size += info.st_size
            latest = max(latest, info.st_mtime_ns)
            count += 1
    return Stamp(size, latest, count)


def stamps(dag: Dag, paths: tuple[str, ...], seen: Stamps | None = None) -> Stamps:
    if seen is None:
        return {path: stamp(dag.path(path), path in dag.folders) for path in paths}
    found = {}
    for path in paths:
        if path not in seen:
            seen[path] = stamp(dag.path(path), path in dag.folders)
        found[path] = seen[path]
    return found


FolderEntries = tuple[tuple[str, int, int], ...]


def folder_entries(root: Path) -> FolderEntries:
    entries = []
    for directory, folders, files in os.walk(root):
        folders.sort()
        for name in folders:
            entries.append((f"d:{(Path(directory) / name).relative_to(root)}", 0, 0))
        for name in sorted(files):
            path = Path(directory) / name
            try:
                info = path.stat()
            except FileNotFoundError:
                continue
            if path.is_file():
                entries.append((f"f:{path.relative_to(root)}", info.st_size, info.st_mtime_ns))
    return tuple(entries)


def folder_inputs(dag: Dag, job: Job) -> dict[str, FolderEntries]:
    return {path: folder_entries(dag.path(path)) for path in job.input_paths if path in dag.folders}


def _file_digest(path: str) -> bytes:
    digest = _new()
    with open(path, "rb") as stream:
        while chunk := stream.read(CHUNK):
            digest.update(chunk)
    return digest.digest()


def content_hash(path: str, folder: bool = False) -> str | None:
    try:
        if not folder:
            return f"{NAME}:{_file_digest(path).hex()}" if os.path.isfile(path) else None
        if not os.path.isdir(path):
            return None
        digest = _new()
        for directory, folders, files in os.walk(path):
            folders.sort()
            for name in folders:
                digest.update(b"d" + os.path.relpath(os.path.join(directory, name), path).encode() + b"\0")
            for name in sorted(files):
                full = os.path.join(directory, name)
                if not os.path.isfile(full):
                    continue
                digest.update(b"f" + os.path.relpath(full, path).encode() + b"\0" + _file_digest(full))
        return f"{NAME}:{digest.hexdigest()}"
    except FileNotFoundError:
        return None


def hashes(dag: Dag, paths: tuple[str, ...]) -> dict[str, str]:
    return {path: h for path in paths if (h := content_hash(str(dag.path(path)), path in dag.folders)) is not None}
