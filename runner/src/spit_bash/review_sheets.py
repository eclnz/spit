from __future__ import annotations

import csv
import fcntl
import io
import os
from pathlib import Path
from tempfile import NamedTemporaryFile

from .gates import ARTIFACT_COLUMN, EVIDENCE_COLUMN
from .model import DagError
from .plan import Decision
from .report.reviews import Readiness, ReviewReport, Waiting


def prepare_sheets(reports: list[ReviewReport], decisions: tuple[Decision, ...]) -> set[Path]:
    selected = {decision.job.id for decision in decisions}
    dimensions: dict[Path, tuple[str, ...]] = {}
    sheets: dict[Path, dict[str, dict[str, str]]] = {}
    for report in reports:
        path = report.review.file
        if path is None or (report.review.found and not report.review.generated):
            continue
        dimensions[path] = report.review.dimensions
        for target in report.targets:
            if not isinstance(target, Waiting) or target.readiness is not Readiness.READY:
                continue
            if target.job is not None and target.job not in selected:
                continue
            identity = target.target.identity
            sheets.setdefault(path, {})[identity] = {
                **dict(target.target.entities),
                ARTIFACT_COLUMN: identity,
                EVIDENCE_COLUMN: str(target.file),
            }

    created = set()
    for path, rows in sheets.items():
        header = (*dimensions[path], ARTIFACT_COLUMN, EVIDENCE_COLUMN, "verdict", "reason")
        try:
            if not path.exists() and _create(path, header, rows):
                created.add(path)
            else:
                _append(path, rows)
        except (OSError, ValueError, csv.Error) as exc:
            raise DagError(f"cannot prepare review file {path}: {exc}") from exc
    return created


def _write(header: tuple[str, ...], rows: dict[str, dict[str, str]], *, include_header: bool) -> bytes:
    text = io.StringIO(newline="")
    writer = csv.DictWriter(text, header, extrasaction="ignore")
    if include_header:
        writer.writeheader()
    writer.writerows(rows[key] for key in sorted(rows))
    return text.getvalue().encode("utf-8")


def _create(path: Path, header: tuple[str, ...], rows: dict[str, dict[str, str]]) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True)
    with NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.") as temporary:
        temporary.write(_write(header, rows, include_header=True))
        temporary.flush()
        try:
            os.link(temporary.name, path)
        except FileExistsError:
            return False
    return True


def _append(path: Path, rows: dict[str, dict[str, str]]) -> None:
    with os.fdopen(os.open(path, os.O_RDWR | os.O_APPEND), "r+b") as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        original = file.read()
        reader = csv.reader(io.StringIO(original.decode("utf-8-sig"), newline=""), strict=True)
        header = tuple(cell.strip() for cell in next(reader, ()))
        if not {ARTIFACT_COLUMN, EVIDENCE_COLUMN, "verdict"}.issubset(header):
            return
        column = header.index(ARTIFACT_COLUMN)
        seen = {cells[column].strip() for cells in reader if len(cells) == len(header)}
        added = {key: row for key, row in rows.items() if key not in seen}
        if added:
            separator = b"" if original.endswith((b"\n", b"\r")) else b"\r\n"
            file.write(separator + _write(header, added, include_header=False))
