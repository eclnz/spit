"""Choose jobs by their outputs, optionally including the jobs they need."""

from __future__ import annotations

from dataclasses import dataclass, replace

from .model import Dag, DagError, Job

LISTED_VALUES = 10

Entities = tuple[tuple[str, frozenset[str]], ...]


@dataclass(frozen=True)
class Selection:
    # A job is chosen when one of its outputs matches every kind of criterion
    # given, and within a kind matches any one of the values given.
    entities: tuple[Entities, ...] = ()
    products: tuple[str, ...] = ()
    stages: tuple[tuple[str, ...], ...] = ()

    def __bool__(self) -> bool:
        return bool(self.entities or self.products or self.stages)


def parse_entities(text: str) -> Entities:
    """Parse one flag, with alternatives per dimension and all dimensions required."""
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


def parse_stage(text: str) -> tuple[str, ...]:
    """`preprocess/combine` as the stage path, outermost first."""
    parts = tuple(text.split("/"))
    if not all(parts):
        raise ValueError(f"expected a stage such as `preprocess` or `preprocess/combine`, got `{text}`")
    return parts


def _listing(names: set[str]) -> str:
    shown = sorted(names)
    more = len(shown) - LISTED_VALUES
    return ", ".join(shown[:LISTED_VALUES]) + (f" and {more} more" if more > 0 else "")


def _check_names(dag: Dag, selection: Selection) -> None:
    """Name a criterion the DAG cannot match at all, so a typo is not just "no job matches"."""
    products = {output.product for job in dag.jobs for output in job.output_artifacts}
    values: dict[str, set[str]] = {}
    for job in dag.jobs:
        for output in job.output_artifacts:
            for dimension, value in output.entities:
                values.setdefault(dimension, set()).add(value)
    stages = {job.stage[:depth] for job in dag.jobs for depth in range(1, len(job.stage) + 1)}
    for product in selection.products:
        if product not in products:
            raise DagError(f"no output is of product `{product}`; products: {_listing(products)}")
    for group in selection.entities:
        for dimension, allowed in group:
            if dimension not in values:
                raise DagError(f"no output has dimension `{dimension}`; dimensions: {_listing(set(values)) or 'none'}")
            unknown = allowed.difference(values[dimension])
            if unknown:
                raise DagError(f"no output has {dimension}={min(unknown)}; {dimension} takes {_listing(values[dimension])}")
    for stage in selection.stages:
        if stage not in stages:
            if not stages:
                raise DagError(f"no job is in stage `{'/'.join(stage)}`: this DAG has no stages")
            listed = {"/".join(path) for path in stages}
            raise DagError(f"no job is in stage `{'/'.join(stage)}`; stages: {_listing(listed)}")


def _matches(job: Job, selection: Selection) -> bool:
    if selection.stages and not any(job.stage[:len(stage)] == stage for stage in selection.stages):
        return False
    for output in job.output_artifacts:
        if selection.products and output.product not in selection.products:
            continue
        if selection.entities:
            entities = dict(output.entities)
            if not any(all(entities.get(dimension) in allowed for dimension, allowed in group)
                       for group in selection.entities):
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
    """Keep matching jobs, using inputs from excluded producers as external files under `no_upstream`."""
    if not selection:
        ids = frozenset(job.id for job in dag.jobs)
        return Selected(dag, len(dag.jobs), 0, ids)
    _check_names(dag, selection)
    matched = {job.id for job in dag.jobs if _matches(job, selection)}
    if not matched:
        raise DagError("no job's outputs match every criterion together")
    if no_upstream:
        jobs = tuple(job for job in dag.jobs if job.id in matched)
        if len(jobs) == len(dag.jobs):
            return Selected(dag, len(jobs), 0, frozenset(matched))
        excluded = {path for job in dag.jobs if job.id not in matched for path in job.output_paths}
        boundary = {}
        kept = []
        for job in jobs:
            if any(dep not in matched for dep in job.depends_on):
                if dag.screens is not None and any(
                        dep not in matched and (dep in dag.screens.after or dep in dag.screens.affected)
                        for dep in job.depends_on):
                    raise DagError(f"job {job.id} needs a screen verdict from an excluded producer. "
                                   "Include its upstream jobs, or use --bypass-verdicts to ignore all verdicts.")
                boundary.update(dict.fromkeys(path for path in job.input_paths if path in excluded))
                job = replace(job, depends_on=tuple(dep for dep in job.depends_on if dep in matched))
            kept.append(job)
        return Selected(replace(dag, jobs=tuple(kept), external_inputs=(*dag.external_inputs, *boundary)),
                        len(matched), 0, frozenset(matched))
    by_id = {job.id: job for job in dag.jobs}
    chosen = set(matched)
    stack = list(matched)
    while stack:
        for dependency in by_id[stack.pop()].depends_on:
            if dependency not in chosen:
                chosen.add(dependency)
                stack.append(dependency)
    jobs = tuple(job for job in dag.jobs if job.id in chosen)
    return Selected(replace(dag, jobs=jobs), len(matched), len(chosen) - len(matched), frozenset(matched))
