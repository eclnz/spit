from __future__ import annotations

import json
from collections import Counter
from collections.abc import Callable, Collection
from pathlib import Path

from ..cause import Why
from ..model import Artifact, Dag
from ..plan import Decision, Status
from ..reuse.identity import claimed_keys, decode_key
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


def _plural(count: int, noun: str) -> str:
    return f"{count} {noun if count == 1 else noun + 's'}"


def _by_product(decisions: tuple[Decision, ...], external: set[str]) -> dict[str, list[Artifact]]:
    every: dict[str, list[Artifact]] = {}
    for decision in decisions:
        for artifact in decision.job.input_artifacts:
            if artifact.path in external:
                every.setdefault(artifact.product, []).append(artifact)
    return every


def _source_parts(
    sources: dict[str, list[Artifact]], every: dict[str, list[Artifact]], label: Callable[[int, str], str]
) -> list[str]:
    found = []
    for product, chosen in sources.items():
        paths = {artifact.path for artifact in chosen}
        described = _described(chosen, [artifact for artifact in every[product] if artifact.path not in paths])
        found.append(label(len(chosen), product) + (f" ({described})" if described else ""))
    return found


def _recorded(
    decisions: tuple[Decision, ...], records: Collection[str], known: Collection[str] | None, external: set[str]
) -> set[str]:
    recorded = {
        artifact.path
        for decision in decisions
        if decision.job.work.key in records
        for artifact in decision.job.input_artifacts
        if artifact.path in external
    }
    return recorded | set(known or ())


def _reran(decisions: tuple[Decision, ...], why: Why) -> list[Decision]:
    return [decision for decision in decisions if decision.status is Status.RUN and decision.cause.why is why]


def _gone_part(everything: Dag, records: Collection[str]) -> list[str]:
    claimed = claimed_keys(everything)
    gone = [key for key in records if key not in claimed]
    if not gone:
        return []
    kept = {pair for job in everything.jobs for artifact in job.output_artifacts for pair in artifact.entities}
    went = sorted(
        {(dimension, value) for key in gone for _, entities in decode_key(key) for dimension, value in entities} - kept
    )
    named = ", ".join(f"{dimension}={value}" for dimension, value in went[:NAMED])
    more = ", ..." if len(went) > NAMED else ""
    shown = f" ({named}{more})" if went else ""
    return [f"{_plural(len(gone), 'job')} from earlier runs no longer in the DAG{shown}"]


def _edited_parts(decisions: tuple[Decision, ...]) -> list[str]:
    edited = Counter(
        (decision.job.operation, parts(decision.cause.parts)) for decision in _reran(decisions, Why.WORK_CHANGED)
    )
    return [f"{operation}: {what} ({_plural(count, 'job')})" for (operation, what), count in sorted(edited.items())]


def change_lines(
    everything: Dag, decisions: tuple[Decision, ...], records: Collection[str], known: Collection[str] | None
) -> list[str]:
    if not records:
        return []
    external = set(everything.external_inputs)
    every = _by_product(decisions, external)
    fresh = [decision for decision in decisions if decision.job.work.key not in records] if known is not None else []
    new = _sources(everything, fresh, _recorded(decisions, records, known, external))
    shown = {artifact.path for chosen in new.values() for artifact in chosen}
    changed = _sources(everything, _reran(decisions, Why.INPUT_CHANGED), shown)
    found = [
        *_source_parts(new, every, lambda count, product: f"+{count} {product}"),
        *_source_parts(changed, every, lambda count, product: f"{count} {product} changed"),
        *_gone_part(everything, records),
        *_edited_parts(decisions),
    ]
    return [f"since the last run: {'; '.join(found)}"] if found else []
