from __future__ import annotations

import json
from collections import Counter
from collections.abc import Collection
from pathlib import Path

from ..cause import Why
from ..model import Artifact, Dag
from ..plan import Decision, Status
from .causes import parts

NAMED = 3


def _entities(artifact: Artifact) -> str:
    return " ".join(f"{dimension}={value}" for dimension, value in artifact.entities)


def _described(chosen: list[Artifact], others: list[Artifact]) -> str:
    if len(chosen) <= NAMED:
        return ", ".join(named for named in map(_entities, chosen) if named)
    seen = {pair for artifact in others for pair in artifact.entities}
    new = sorted({pair for artifact in chosen for pair in artifact.entities if pair not in seen})
    return ", ".join(f"{dimension}={value}" for dimension, value in new)


def _sources(dag: Dag, chosen: list[Decision], exclude: set[str]) -> dict[str, list[Artifact]]:
    external = set(dag.external_inputs)
    found: dict[str, dict[str, Artifact]] = {}
    for decision in chosen:
        for artifact in decision.job.input_artifacts:
            if artifact.path in external and artifact.path not in exclude:
                found.setdefault(artifact.product, {})[artifact.path] = artifact
    return {product: sorted(by_path.values(), key=lambda a: a.path) for product, by_path in sorted(found.items())}


def saved_sources(path: Path) -> frozenset[str] | None:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return frozenset(item for item in data if isinstance(item, str)) if isinstance(data, list) else None


def save_sources(path: Path, dag: Dag, kept: Collection[str] = ()) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(sorted({*dag.external_inputs, *kept}), indent=1) + "\n", encoding="utf-8")
    temporary.replace(path)


def change_lines(
    everything: Dag, decisions: tuple[Decision, ...], records: Collection[str], known: Collection[str] | None
) -> list[str]:
    if not records:
        return []
    external = set(everything.external_inputs)
    fresh = [decision for decision in decisions if decision.job.work.key not in records] if known is not None else []
    recorded = {
        artifact.path
        for decision in decisions
        if decision.job.work.key in records
        for artifact in decision.job.input_artifacts
        if artifact.path in external
    }
    recorded |= set(known or ())
    every: dict[str, list[Artifact]] = {}
    for decision in decisions:
        for artifact in decision.job.input_artifacts:
            if artifact.path in external:
                every.setdefault(artifact.product, []).append(artifact)
    parts_of: list[str] = []
    new = _sources(everything, fresh, recorded)
    for product, chosen in new.items():
        paths = {artifact.path for artifact in chosen}
        others = [artifact for artifact in every[product] if artifact.path not in paths]
        described = _described(chosen, others)
        parts_of.append(f"+{len(chosen)} {product}" + (f" ({described})" if described else ""))
    inputs_changed = [
        decision for decision in decisions if decision.status is Status.RUN and decision.cause.why is Why.INPUT_CHANGED
    ]
    changed = _sources(everything, inputs_changed, {a.path for chosen in new.values() for a in chosen})
    for product, chosen in changed.items():
        paths = {artifact.path for artifact in chosen}
        others = [artifact for artifact in every[product] if artifact.path not in paths]
        described = _described(chosen, others)
        parts_of.append(f"{len(chosen)} {product} changed" + (f" ({described})" if described else ""))
    claimed = {job.work.key for job in everything.jobs} | {job.key for job in everything.gated}
    gone = [key for key in records if key not in claimed]
    if gone:
        kept = {pair for job in everything.jobs for artifact in job.output_artifacts for pair in artifact.entities}
        went = sorted(
            {(dimension, value) for key in gone for _, entities in json.loads(key) for dimension, value in entities}
            - kept
        )
        named = (
            f" ({', '.join(f'{d}={v}' for d, v in went[:NAMED])}{', ...' if len(went) > NAMED else ''})" if went else ""
        )
        parts_of.append(
            f"{len(gone)} {'job' if len(gone) == 1 else 'jobs'} from earlier runs no longer in the DAG{named}"
        )
    edited = Counter(
        (decision.job.operation, parts(decision.cause.parts))
        for decision in decisions
        if decision.status is Status.RUN and decision.cause.why is Why.WORK_CHANGED
    )
    for (operation, what), count in sorted(edited.items()):
        parts_of.append(f"{operation}: {what} ({count} {'job' if count == 1 else 'jobs'})")
    return [f"since the last run: {'; '.join(parts_of)}"] if parts_of else []
