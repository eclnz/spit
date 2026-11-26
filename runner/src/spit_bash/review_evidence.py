from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from graphlib import CycleError, TopologicalSorter
from typing import TYPE_CHECKING

from . import schema
from .model import DagError

if TYPE_CHECKING:
    from .gates import Producers


@dataclass(frozen=True)
class Read:
    job: int
    port: int


@dataclass(frozen=True)
class Evidence:
    makers: tuple[int, ...]
    gated_reads: tuple[Read, ...]


def bind(
    document: schema.SpitDag, producers: Producers, artifacts: dict[str, schema.Artifact]
) -> dict[tuple[str, str], Evidence]:
    targets = [
        (review.name, target.artifact, target.evidence)
        for review in document.reviews
        for target in review.targets
        if target.evidence is not None
    ]
    if not targets:
        return {}
    jobs = {job.id: job for job in document.jobs}
    readers: dict[str, list[Read]] = defaultdict(list)
    reviewed = {artifact.path for _, artifact, _ in targets}
    for job in document.jobs:
        for port, inputs in enumerate(job.inputs.values()):
            for artifact in inputs:
                for path in dict.fromkeys((artifact.path, *producers.paths_of(artifact))):
                    if path is not None and path in reviewed:
                        readers[path].append(Read(job.id, port))
    ancestors: dict[tuple[int, ...], set[int]] = {}
    bounds: dict[tuple[str, str], Evidence] = {}
    graph = {job.id: set(job.depends_on) for job in document.jobs}
    for name, candidate, evidence in targets:
        assert evidence is not None
        for artifact in (candidate, evidence):
            if artifacts.get(artifact.path) != artifact:
                raise DagError(f"review {name} names an unknown or inconsistent artifact {artifact.path}")
        if any(candidate.entities.get(key) != value for key, value in evidence.entities.items()):
            raise DagError(f"review {name} evidence {evidence.path} does not match entities of {candidate.path}")
        key = (name, candidate.path)
        if key in bounds:
            raise DagError(f"review {name} declares evidence twice for {candidate.path}")
        makers = tuple(
            sorted(
                {
                    producers.paths[path]
                    for path in producers.paths_of(evidence)
                    if path is not None and path in producers.paths
                }
            )
        )
        if makers not in ancestors:
            upstream: set[int] = set()
            pending = list(makers)
            while pending:
                job_id = pending.pop()
                if job_id not in upstream:
                    upstream.add(job_id)
                    pending.extend(jobs[job_id].depends_on)
            ancestors[makers] = upstream
        gated = tuple(dict.fromkeys(read for read in readers[candidate.path] if read.job not in ancestors[makers]))
        for read in gated:
            graph[read.job].update(makers)
        bounds[key] = Evidence(makers, gated)
    try:
        TopologicalSorter(graph).prepare()
    except CycleError as exc:
        path = " -> ".join(map(str, exc.args[1]))
        raise DagError(f"review evidence dependency cycle between jobs {path}") from exc
    return bounds
