from __future__ import annotations

import pickle
from dataclasses import dataclass, replace
from pathlib import Path

from ..execute.options import Options
from ..model import Dag, Job
from . import atomic
from .reconcile import Mode


@dataclass(frozen=True)
class Spec:
    job: int
    mode: Mode
    log: Path
    reason: str | None = None
    keep_edited: bool = False
    expected: tuple[tuple[str, tuple[int, ...]], ...] = ()
    attempt: int | None = None
    memory: int | None = None
    time_limit: float | None = None


@dataclass(frozen=True)
class Batch:
    dag: Dag
    options: Options
    tasks: tuple[Spec, ...]
    intent: str
    folder: Path
    visibility: float

    def job(self, spec: Spec) -> Job:
        return next(job for job in self.dag.jobs if job.id == spec.job)


def trim(dag: Dag, jobs: list[Job], neighbours: dict[Path, list[str]]) -> Dag:
    paths = {path for job in jobs for path in (*job.input_paths, *job.output_paths)}
    for job in jobs:
        for output in job.output_paths:
            paths.update(neighbours.get(dag.path(output).parent, ()))
    for job in jobs:
        for check in job.checks:
            paths.add(check.path)
    return replace(
        dag,
        jobs=tuple(jobs),
        absolute={path: dag.absolute[path] for path in paths if path in dag.absolute},
        folders=dag.folders & paths,
        in_dataset=dag.in_dataset & paths,
        in_pipeline=dag.in_pipeline & paths,
        external_inputs=(),
        executables=(),
        left_out=(),
        reviews=(),
        gated=(),
        screens=None,
    )


def by_folder(dag: Dag) -> dict[Path, list[str]]:
    found: dict[Path, list[str]] = {}
    for path, absolute in dag.absolute.items():
        found.setdefault(absolute.parent, []).append(path)
    return found


def write(batch: Batch, path: Path) -> None:
    with atomic.writing(path) as stream:
        pickle.dump(batch, stream, protocol=pickle.HIGHEST_PROTOCOL)


def read(path: Path) -> Batch:
    with path.open("rb") as stream:
        batch = pickle.load(stream)
    if not isinstance(batch, Batch):
        raise ValueError(f"{path} holds no batch")
    return batch
