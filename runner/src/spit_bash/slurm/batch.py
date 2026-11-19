"""What the controller hands the tasks of one Slurm array: the DAG cut down to their jobs, each task, and the
run's options."""

from __future__ import annotations

import os
import pickle
import tempfile
from dataclasses import dataclass, replace
from pathlib import Path

from ..execute.options import Options
from ..model import Dag, Job
from .reconcile import Mode


@dataclass(frozen=True)
class Spec:
    """One task of an array: its job, and how to run it."""

    job: int
    mode: Mode
    # The job's log.
    log: Path
    # The plan's words for why the job runs, which start its log.
    reason: str | None = None
    # Whether it reruns because its output was edited, so a success keeps the edited output beside it.
    keep_edited: bool = False
    # Each input another job made, with the stamp that job recorded, as the state stores it: the task waits until its
    # node sees each so, or until `Batch.visibility` after the input was written, before it starts.
    expected: tuple[tuple[str, tuple[int, ...]], ...] = ()


@dataclass(frozen=True)
class Batch:
    """The tasks of one array, by array index, with the DAG cut down to their jobs."""

    dag: Dag
    options: Options
    tasks: tuple[Spec, ...]
    # The intent's name, which the task's result repeats.
    intent: str
    # The runner folder's Slurm folder, which holds the results and the claims.
    folder: Path
    # How long a file one node wrote may stay unseen on another: the shared filesystem's visibility bound.
    visibility: float

    def job(self, spec: Spec) -> Job:
        return next(job for job in self.dag.jobs if job.id == spec.job)


def trim(dag: Dag, jobs: list[Job], neighbours: dict[Path, list[str]]) -> Dag:
    """`dag` with only `jobs`, and only the paths they read and write.

    `neighbours` lists the DAG's paths by the folder they are in. The paths in
    the folders of the jobs' outputs are kept too, so a job whose tool wrote a
    file under a name of its own can tell that file from another job's output.
    Reviews and screens are settled and reported by the controller.
    """
    paths = {path for job in jobs for path in (*job.input_paths, *job.output_paths)}
    for job in jobs:
        for output in job.output_paths:
            paths.update(neighbours.get(dag.path(output).parent, ()))
    for job in jobs:
        for check in job.checks:
            paths.add(check.path)
    return replace(dag, jobs=tuple(jobs), absolute={path: dag.absolute[path] for path in paths if path in dag.absolute},
                   folders=dag.folders & paths, in_dataset=dag.in_dataset & paths, in_pipeline=dag.in_pipeline & paths,
                   external_inputs=(), executables=(), left_out=(), reviews=(), gated=(), screens=None)


def by_folder(dag: Dag) -> dict[Path, list[str]]:
    """The DAG's paths by the folder each is in, for `trim`."""
    found: dict[Path, list[str]] = {}
    for path, absolute in dag.absolute.items():
        found.setdefault(absolute.parent, []).append(path)
    return found


def write(batch: Batch, path: Path) -> None:
    """Write `batch` where every node can read it whole: to a temporary name, synced, then renamed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            pickle.dump(batch, stream, protocol=pickle.HIGHEST_PROTOCOL)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read(path: Path) -> Batch:
    """The batch at `path`, which the same spit-bash wrote: it holds the run's own objects, not data from a user."""
    with path.open("rb") as stream:
        batch = pickle.load(stream)
    if not isinstance(batch, Batch):
        raise ValueError(f"{path} holds no batch")
    return batch
