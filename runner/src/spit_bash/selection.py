from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass, replace

from .model import Admission, Dag, DagError, GatedJob, Job

LISTED_VALUES = 10

Entities = tuple[tuple[str, frozenset[str]], ...]
ForceSelection = bool | frozenset[int]


def is_forced(force: ForceSelection, job_id: int) -> bool:
    return force if isinstance(force, bool) else job_id in force


@dataclass(frozen=True)
class Selection:
    entities: tuple[Entities, ...] = ()
    products: tuple[str, ...] = ()
    stages: tuple[tuple[str, ...], ...] = ()

    def __bool__(self) -> bool:
        return bool(self.entities or self.products or self.stages)


def parse_entities(text: str) -> Entities:
    values: dict[str, set[str]] = {}
    dimension = ""
    for item in text.split(","):
        key, equals, value = item.partition("=")
        if equals:
            if not key or not value:
                raise ValueError(f"expected DIMENSION=VALUE, got `{item}`")
            if key in values:
                raise ValueError(f"dimension `{key}` given twice")
            dimension = key
            values[dimension] = {value}
        elif not dimension:
            raise ValueError(f"expected DIMENSION=VALUE, got `{item}`")
        elif not item:
            raise ValueError(f"expected a value for dimension `{dimension}` after comma")
        else:
            values[dimension].add(item)
    return tuple((key, frozenset(allowed)) for key, allowed in sorted(values.items()))


def _entries(text: str, kind: str) -> list[str]:
    entries = text.split(",")
    if not all(entries):
        raise ValueError(f"expected a {kind} between each comma, got `{text}`")
    return entries


def parse_products(text: str) -> tuple[str, ...]:
    return tuple(_entries(text, "product"))


def parse_stage(text: str) -> tuple[str, ...]:
    parts = tuple(text.split("/"))
    if not all(parts):
        raise ValueError(f"expected a stage such as `preprocess` or `preprocess/combine`, got `{text}`")
    return parts


def parse_stages(text: str) -> tuple[tuple[str, ...], ...]:
    return tuple(parse_stage(entry) for entry in _entries(text, "stage"))


def _listing(names: set[str]) -> str:
    shown = sorted(names)
    more = len(shown) - LISTED_VALUES
    return ", ".join(shown[:LISTED_VALUES]) + (f" and {more} more" if more > 0 else "")


def _outputs(job: Job | GatedJob) -> Iterable[tuple[str, tuple[tuple[str, str], ...]]]:
    if isinstance(job, GatedJob):
        return job.contexts
    return ((artifact.product, artifact.entities) for artifact in job.output_artifacts)


def _check_names(jobs: tuple[Job | GatedJob, ...], selection: Selection) -> None:
    products: set[str] = set()
    values: dict[str, set[str]] = {}
    for job in jobs:
        for product, entities in _outputs(job):
            products.add(product)
            for dimension, value in entities:
                values.setdefault(dimension, set()).add(value)
    stages = {job.stage[:depth] for job in jobs for depth in range(1, len(job.stage) + 1)}
    for product in selection.products:
        if product not in products:
            raise DagError(f"no output is of product `{product}`; products: {_listing(products)}")
    for group in selection.entities:
        for dimension, allowed in group:
            if dimension not in values:
                raise DagError(f"no output has dimension `{dimension}`; dimensions: {_listing(set(values)) or 'none'}")
            unknown = allowed.difference(values[dimension])
            if unknown:
                raise DagError(
                    f"no output has {dimension}={min(unknown)}; {dimension} takes {_listing(values[dimension])}"
                )
    for stage in selection.stages:
        if stage not in stages:
            if not stages:
                raise DagError(f"no job is in stage `{'/'.join(stage)}`: this DAG has no stages")
            listed = {"/".join(path) for path in stages}
            raise DagError(f"no job is in stage `{'/'.join(stage)}`; stages: {_listing(listed)}")


def _matches(job: Job | GatedJob, selection: Selection) -> bool:
    if selection.stages and not any(job.stage[: len(stage)] == stage for stage in selection.stages):
        return False
    for product, context in _outputs(job):
        if selection.products and product not in selection.products:
            continue
        if selection.entities:
            entities = dict(context)
            if not any(
                all(entities.get(dimension) in allowed for dimension, allowed in group) for group in selection.entities
            ):
                continue
        return True
    return False


@dataclass(frozen=True)
class Selected:
    dag: Dag
    matched: int
    needed: int
    matched_ids: frozenset[int]


def select(dag: Dag, selection: Selection, *, no_upstream: bool = False) -> Selected:
    if not selection:
        ids = frozenset(job.id for job in dag.jobs)
        return Selected(dag, len(dag.jobs), 0, ids)
    gated = tuple(job for job in dag.gated if job.contexts)
    selectable: tuple[Job | GatedJob, ...] = (*dag.jobs, *gated)
    _check_names(selectable, selection)
    matched = {job.id for job in selectable if _matches(job, selection)}
    if not matched:
        raise DagError("no job's outputs match every criterion together")
    if no_upstream:
        jobs = tuple(job for job in dag.jobs if job.id in matched)
        if len(jobs) == len(dag.jobs) and not gated:
            return Selected(dag, len(jobs), 0, frozenset(matched))
        excluded = {path for job in dag.jobs if job.id not in matched for path in job.output_paths}
        boundary = {}
        kept = []
        for job in jobs:
            if any(dep not in matched for dep in job.depends_on):
                if dag.screens is not None and any(
                    dep not in matched and (dep in dag.screens.after or dep in dag.screens.affected)
                    for dep in job.depends_on
                ):
                    raise DagError(
                        f"job {job.id} needs a screen verdict from an excluded producer. "
                        "Include its upstream jobs, or use --bypass-verdicts to ignore all verdicts."
                    )
                boundary.update(dict.fromkeys(path for path in job.input_paths if path in excluded))
                job = replace(job, depends_on=tuple(dep for dep in job.depends_on if dep in matched))
            kept.append(job)
        return Selected(
            replace(
                dag,
                jobs=tuple(kept),
                external_inputs=(*dag.external_inputs, *boundary),
                gated=tuple(job for job in dag.gated if not gated or job.id in matched),
            ),
            len(matched),
            0,
            frozenset(matched),
        )
    by_id = {job.id: job for job in selectable}
    chosen = set(matched)
    stack = list(matched)
    while stack:
        job_id = stack.pop()
        candidate = by_id[job_id]
        if isinstance(candidate, GatedJob) and candidate.status is Admission.REJECTED:
            continue
        for dependency in (*candidate.depends_on, *dag.review_needs.get(job_id, ())):
            if dependency not in chosen:
                chosen.add(dependency)
                stack.append(dependency)
    jobs = tuple(job for job in dag.jobs if job.id in chosen)
    selected_gates = tuple(job for job in dag.gated if not gated or job.id in chosen)
    return Selected(
        replace(dag, jobs=jobs, gated=selected_gates),
        len(matched),
        len(chosen) - len(matched),
        frozenset(matched),
    )
