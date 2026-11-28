from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from hashlib import sha256
from graphlib import CycleError, TopologicalSorter
from typing import TYPE_CHECKING

from . import schema
from .files import content_hash
from .model import Dag, DagError

if TYPE_CHECKING:
    from .gates import Producers


def revision(dag: Dag, paths: tuple[str, ...], hashes: dict[str, str | None]) -> str | None:
    digest = sha256()
    for path in paths:
        if path not in hashes:
            hashes[path] = content_hash(str(dag.path(path)), path in dag.folders)
        value = hashes[path]
        if value is None:
            return None
        digest.update(path.encode("utf-8") + b"\0" + value.encode("ascii") + b"\0")
    return digest.hexdigest()


@dataclass(frozen=True)
class Read:
    job: int
    port: int


@dataclass(frozen=True)
class Evidence:
    makers: tuple[int, ...]
    gated_reads: tuple[Read, ...]
    evidence_makers: tuple[tuple[int, ...], ...]


def bind(
    document: schema.SpitDag, producers: Producers, artifacts: dict[str, schema.Artifact]
) -> dict[tuple[str, str], Evidence]:
    targets = [
        (review.name, target.artifact, target.evidence_artifacts)
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
        for artifact in (candidate, *evidence):
            if artifacts.get(artifact.path) != artifact:
                raise DagError(f"review {name} names an unknown or inconsistent artifact {artifact.path}")
        if len({artifact.path for artifact in evidence}) != len(evidence):
            raise DagError(f"review {name} declares an evidence artifact twice for {candidate.path}")
        for artifact in evidence:
            if any(candidate.entities.get(key) != value for key, value in artifact.entities.items()):
                raise DagError(f"review {name} evidence {artifact.path} does not match entities of {candidate.path}")
        key = (name, candidate.path)
        if key in bounds:
            raise DagError(f"review {name} declares evidence twice for {candidate.path}")
        evidence_makers = tuple(
            tuple(
                sorted(
                    {
                        producers.paths[path]
                        for path in producers.paths_of(artifact)
                        if path is not None and path in producers.paths
                    }
                )
            )
            for artifact in evidence
        )
        makers = tuple(sorted({maker for group in evidence_makers for maker in group}))
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
        bounds[key] = Evidence(makers, gated, evidence_makers)
    try:
        TopologicalSorter(graph).prepare()
    except CycleError as exc:
        path = " -> ".join(map(str, exc.args[1]))
        raise DagError(f"review evidence dependency cycle between jobs {path}") from exc
    return bounds
