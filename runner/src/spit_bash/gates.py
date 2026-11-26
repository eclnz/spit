from __future__ import annotations

import csv
from collections import defaultdict
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from . import review_evidence, schema
from .model import Admission, AppliedVerdict, DagError, Review, Target

ARTIFACT_COLUMN = "@artifact"
EVIDENCE_COLUMN = "@evidence"


class Verdict(str, Enum):
    PASS = "pass"
    FAIL = "fail"


@dataclass(frozen=True)
class Row:
    line: int
    verdict: Verdict
    reason: str | None


@dataclass(frozen=True)
class Sheet:
    found: bool
    groups: dict[tuple[str, ...], dict[tuple[str, ...], list[Row]]]
    generated: bool = False

    def matching(self, entities: dict[str, str], artifact: str) -> list[Row]:
        return sorted(
            (
                row
                for dimensions, group in self.groups.items()
                for row in group.get(
                    tuple(artifact if name == ARTIFACT_COLUMN else entities.get(name, "") for name in dimensions), ()
                )
            ),
            key=lambda row: row.line,
        )


def _read(path: Path, dimensions: list[str]) -> Sheet:
    groups: dict[tuple[str, ...], dict[tuple[str, ...], list[Row]]] = {}
    line = 1
    try:
        with path.open(encoding="utf-8-sig", newline="") as source:
            reader = csv.reader(source, strict=True)
            header = [cell.strip() for cell in next(reader, [])]
            if "verdict" not in header or len(set(header)) != len(header):
                raise ValueError("expected a header row with distinct columns including `verdict`")
            generated = ARTIFACT_COLUMN in header and EVIDENCE_COLUMN in header
            metadata = (ARTIFACT_COLUMN, EVIDENCE_COLUMN) if generated else ()
            for column in header:
                if column not in ("verdict", "reason", *metadata) and column not in dimensions:
                    raise ValueError(f"column {column!r} is not a dimension of every product this review judges")
            while True:
                line = reader.line_num + 1
                cells = next(reader, None)
                if cells is None:
                    break
                if not cells or (len(cells) == 1 and not cells[0].strip()):
                    continue
                if len(cells) != len(header):
                    raise ValueError(f"expected {len(header)} cells, found {len(cells)}")
                fields = dict(zip(header, (cell.strip() for cell in cells)))
                word = fields.pop("verdict")
                if not word:
                    continue
                if word not in ("pass", "fail"):
                    raise ValueError(f"verdict `{word}` must be `pass` or `fail`")
                reason = " ".join(fields.pop("reason", "").split()) or None
                if generated:
                    identity = fields[ARTIFACT_COLUMN]
                    if not identity:
                        raise ValueError(f"a verdict must name an artifact in `{ARTIFACT_COLUMN}`")
                    names, key = (ARTIFACT_COLUMN,), (identity,)
                else:
                    names = tuple(sorted(name for name, cell in fields.items() if cell))
                    if not names:
                        raise ValueError("row names no dimension value")
                    key = tuple(fields[name] for name in names)
                    if any(any(character.isspace() for character in value) for value in key):
                        raise ValueError("dimension values must each be one token")
                groups.setdefault(names, {}).setdefault(key, []).append(Row(line, Verdict(word), reason))
    except FileNotFoundError:
        return Sheet(False, {})
    except (OSError, ValueError, csv.Error) as exc:
        raise DagError(f"review file {path} line {line}: {exc}") from exc
    _check_conflicts(path, groups)
    return Sheet(True, groups, generated)


def _check_conflicts(path: Path, groups: dict[tuple[str, ...], dict[tuple[str, ...], list[Row]]]) -> None:
    patterns = list(groups.items())
    for index, (left_names, left) in enumerate(patterns):
        for right_names, right in patterns[index:]:
            shared = tuple(name for name in left_names if name in right_names)
            right_indices = tuple(right_names.index(name) for name in shared)
            left_indices = tuple(left_names.index(name) for name in shared)
            seen: dict[tuple[tuple[str, ...], Verdict], Row] = {}
            for values, rows in left.items():
                key = tuple(values[i] for i in left_indices)
                for row in rows:
                    seen.setdefault((key, row.verdict), row)
            for values, rows in right.items():
                key = tuple(values[i] for i in right_indices)
                for row in rows:
                    opposite = Verdict.FAIL if row.verdict is Verdict.PASS else Verdict.PASS
                    other = seen.get((key, opposite))
                    if other is not None:
                        earlier, later = sorted((row, other), key=lambda item: item.line)
                        raise DagError(
                            f"review file {path} line {later.line}: verdict {later.verdict!r} "
                            f"conflicts with {earlier.verdict!r} on line {earlier.line}"
                        )


@dataclass(frozen=True)
class Scope:
    path: str
    job: int | None = None
    port: int | None = None


@dataclass(frozen=True)
class ReviewDecisions:
    scopes: dict[Scope, Admission]
    reports: tuple[Review, ...]
    overrides: dict[Scope, Row]
    evidence_needs: dict[int, set[int]]
    explicit_evidence: bool = False


def _identity(artifact: schema.Artifact) -> str:
    values = ",".join(f"{name}={value}" for name, value in artifact.entities.items())
    return f"{artifact.product}[{values}]" if values else artifact.product


def read_reviews(document: schema.SpitDag, screened: set[Scope], producers: Producers) -> ReviewDecisions:
    scopes: dict[Scope, Admission] = {}
    reports: list[Review] = []
    overrides: dict[Scope, Row] = {}
    sheets: dict[Path, tuple[list[str], Sheet]] = {}
    jobs = {job.id: job for job in document.jobs}
    artifacts = {artifact.path: artifact for job in document.jobs for artifact in job.outputs.values()}
    artifacts.update((artifact.path, artifact) for artifact in document.external_inputs)
    artifacts.update((artifact.path, artifact) for artifact in document.targets)
    for job in document.jobs:
        for port in job.inputs.values():
            artifacts.update((artifact.path, artifact) for artifact in port)
    evidence = review_evidence.bind(document, producers, artifacts)
    needs: dict[int, set[int]] = defaultdict(set)
    dimensions_by_file: dict[Path, set[str]] = {}
    for review in document.reviews:
        if review.file is not None:
            path = Path(review.file)
            if not path.is_absolute():
                raise DagError(f"review {review.name} needs an absolute file path")
            path = path.resolve()
            dimensions_by_file.setdefault(path, set(review.dimensions)).intersection_update(review.dimensions)
    for review in document.reviews:
        path = None if review.file is None else Path(review.file).resolve()
        if path is not None and path not in sheets:
            dimensions = sorted(dimensions_by_file[path])
            sheets[path] = (dimensions, _read(path, dimensions))
        sheet = Sheet(False, {}) if path is None else sheets[path][1]
        unmatched = {row.line for group in sheet.groups.values() for rows in group.values() for row in rows}
        count = len(unmatched)
        targets = []
        applied = []
        for target in review.targets:
            artifact = target.artifact
            identity = _identity(artifact)
            port = None
            if target.input is None:
                if artifacts.get(artifact.path, artifact) != artifact:
                    raise DagError(f"review {review.name} target disagrees with artifact {artifact.path}")
                scope = Scope(artifact.path)
            else:
                use = target.input
                job = jobs.get(use.job)
                ports = list(job.inputs.items()) if job is not None else []
                if job is None or use.port >= len(ports) or artifact not in ports[use.port][1]:
                    raise DagError(f"review {review.name} names an invalid input of job {use.job}")
                port = (job.operation, ports[use.port][0])
                scope = Scope(artifact.path, job.id, use.port)
            rows = sheet.matching(artifact.entities, identity)
            unmatched.difference_update(row.line for row in rows)
            row = rows[0] if rows else None
            bound = None if target.evidence is None else evidence[(review.name, artifact.path)]
            admission = (
                (Admission.OPEN if bound is None and scope in screened else Admission.HELD)
                if row is None
                else (Admission.OPEN if row.verdict is Verdict.PASS else Admission.REJECTED)
            )
            if bound is None:
                scopes[scope] = max(admission, scopes.get(scope, Admission.OPEN))
                if row is not None and (scope not in overrides or row.verdict is Verdict.FAIL):
                    overrides[scope] = row
            else:
                for read in bound.gated_reads:
                    key = Scope(artifact.path, read.job, read.port)
                    scopes[key] = max(admission, scopes.get(key, Admission.OPEN))
                    if admission is Admission.HELD:
                        needs[read.job].update(bound.makers)
            if row is not None:
                applied.append(AppliedVerdict(identity, scope.job, scope.port, row.verdict.value, row.line, row.reason))
            if admission is not Admission.OPEN:
                targets.append(
                    Target(
                        identity,
                        artifact.path,
                        artifact.under == "root",
                        port,
                        admission is Admission.REJECTED,
                        None if row is None else row.reason,
                        None if row is None else row.line,
                        artifact.under == "pipeline_folder",
                        tuple(artifact.entities.items()),
                        None if target.evidence is None else target.evidence.path,
                        () if bound is None else bound.makers,
                    )
                )
        reports.append(
            Review(
                review.name,
                path,
                sheet.found,
                count,
                tuple(sorted(unmatched)),
                tuple(targets),
                tuple(applied),
                () if path is None else tuple(sheets[path][0]),
                sheet.generated,
            )
        )
    return ReviewDecisions(scopes, tuple(reports), overrides, dict(needs), bool(evidence))


def _command(job: schema.SpitJob, command: schema.Command, keep: dict[int, set[int]]) -> schema.Command:
    result = []
    ports = tuple(job.inputs.values())
    for argument in command:
        parts = []
        for part in argument:
            if isinstance(part, schema.MemberPart):
                members = keep.get(part.port)
                if members is None or part.offset >= len(ports[part.port]) or len(argument) != 1:
                    raise DagError(f"job {job.id} has an invalid member argument")
                expected = ports[part.port][part.offset].path
                if isinstance(part.member, schema.PathPart) and part.member.path != expected:
                    raise DagError(
                        f"job {job.id}: member argument for port {part.port} offset {part.offset} "
                        f"names {part.member.path}, expected {expected}"
                    )
                if part.offset in members:
                    parts.append(part.member)
            elif (
                isinstance(part, (schema.JoinPart, schema.ListPart, schema.JoinedValuesPart)) and part.port is not None
            ):
                members = keep.get(part.port)
                values = (
                    part.join
                    if isinstance(part, schema.JoinPart)
                    else (part.list if isinstance(part, schema.ListPart) else part.join_values)
                )
                if members is None or len(values) != len(ports[part.port]):
                    raise DagError(f"job {job.id} has an invalid collection argument")
                if isinstance(part, (schema.JoinPart, schema.ListPart)):
                    for offset, (path, artifact) in enumerate(zip(values, ports[part.port])):
                        if path != artifact.path:
                            kind = "join" if isinstance(part, schema.JoinPart) else "list"
                            raise DagError(
                                f"job {job.id}: {kind} argument for port {part.port} offset {offset} "
                                f"names {path}, expected {artifact.path}"
                            )
                selected = [value for index, value in enumerate(values) if index in members]
                if isinstance(part, schema.JoinPart):
                    parts.append(schema.JoinPart(join=selected, separator=part.separator))
                elif isinstance(part, schema.ListPart):
                    parts.append(schema.ListPart(list=selected))
                else:
                    parts.append(part.separator.join(selected))
            elif isinstance(part, (schema.JoinPart, schema.ListPart)) and job.collection is not None:
                raise DagError(f"job {job.id}: a collection join or list argument must name its input port")
            else:
                parts.append(part)
        if parts:
            result.append(parts)
    if not result:
        raise DagError(f"job {job.id} has an empty command")
    return result


@dataclass(frozen=True)
class Effective:
    document: schema.SpitDag
    excluded: tuple[tuple[int, Admission], ...]
    affected: frozenset[int]


@dataclass(frozen=True)
class Producers:
    paths: dict[str, int]
    ids: dict[int, str]

    def paths_of(self, artifact: schema.Artifact) -> tuple[str | None, ...]:
        if artifact.members is None:
            return (artifact.path,)
        return tuple(self.ids.get(member.id) for member in artifact.members)


def producers_of(jobs: list[schema.SpitJob]) -> Producers:
    producers: dict[str, int] = {}
    ids: dict[int, str] = {}
    seen: set[int] = set()
    for job in jobs:
        if job.id in seen or len(set(job.depends_on)) != len(job.depends_on) or not set(job.depends_on) <= seen:
            raise DagError(f"job {job.id} has a duplicate ID or invalid dependencies")
        seen.add(job.id)
        for artifact in job.outputs.values():
            if artifact.path in producers:
                raise DagError(f"multiple jobs claim path: {artifact.path}")
            producers[artifact.path] = job.id
            if artifact.id is not None:
                ids[artifact.id] = artifact.path

    return Producers(producers, ids)


def effective(
    document: schema.SpitDag, scopes: dict[Scope, Admission], producers: Producers, before: set[int], after: set[int]
) -> Effective:
    excluded: dict[int, Admission] = {}
    affected: set[int] = set()
    unsettled: set[int] = set()
    jobs = []
    for job in document.jobs:
        pending = job.id in before or not unsettled.isdisjoint(job.depends_on)
        selected = select_job(job, scopes, excluded, producers, include_held=pending)
        if isinstance(selected, Admission):
            excluded[job.id] = selected
        else:
            jobs.append(selected)
            if pending:
                affected.add(job.id)
            if pending or job.id in after:
                unsettled.add(job.id)
    return Effective(document.model_copy(update={"jobs": jobs}), tuple(excluded.items()), frozenset(affected))


def select_job(
    job: schema.SpitJob,
    scopes: dict[Scope, Admission],
    excluded: dict[int, Admission],
    makers: Producers,
    *,
    include_held: bool = False,
) -> schema.SpitJob | Admission:
    producers = makers.paths
    if job.collection is None and not scopes and not excluded:
        return job
    collection = job.collection
    ports = tuple(job.inputs.values())
    many = set() if collection is None else set(collection.ports)
    if collection is not None and (len(many) != len(collection.ports) or max(many) >= len(ports)):
        raise DagError(f"job {job.id} has invalid collection ports")
    dependency = Admission.OPEN
    pairs: dict[tuple[str, ...], Admission] = defaultdict(lambda: Admission.OPEN)
    members: dict[int, list[tuple[str, ...]]] = {}
    available: dict[int, set[int]] = defaultdict(set)
    input_producers = set()
    for port, artifacts in enumerate(ports):
        keys = []
        for offset, artifact in enumerate(artifacts):
            paths = makers.paths_of(artifact)
            required = {producers[path] for path in paths if path in producers}
            input_producers.update(required)
            status = max(
                scopes.get(Scope(artifact.path), Admission.OPEN),
                scopes.get(Scope(artifact.path, job.id, port), Admission.OPEN),
                *(scopes.get(Scope(path), Admission.OPEN) for path in paths if path is not None),
                *(
                    scopes.get(Scope(path, job.id, port), Admission.OPEN)
                    for path in paths
                    if path is not None and path != artifact.path
                ),
                *(excluded.get(dep, Admission.OPEN) for dep in required),
            )
            if collection is None or port not in many:
                dependency = max(dependency, status)
            else:
                try:
                    key = tuple(artifact.entities[name] for name in collection.varied)
                except KeyError as exc:
                    raise DagError(f"job {job.id} has a member missing dimension {exc}") from exc
                keys.append(key)
                pairs[key] = max(pairs[key], status)
                if include_held and required.isdisjoint(excluded):
                    available[port].add(offset)
        if port in many:
            members[port] = keys
    if input_producers != set(job.depends_on):
        raise DagError(f"job {job.id} dependencies do not match its input producers")
    if collection is not None:
        incomplete = {tuple(gap.member) for gap in job.missing if gap.member is not None}
        if any((pairs.keys() - set(keys)) - incomplete or len(set(keys)) != len(keys) for keys in members.values()):
            raise DagError(f"job {job.id} has mismatched collection members")
        if len(pairs) < collection.minimum:
            raise DagError(f"job {job.id} has too few potential collection members")
        possible = sum(status is not Admission.REJECTED for status in pairs.values())
        if possible < collection.minimum:
            dependency = Admission.REJECTED
        elif not include_held and any(status is Admission.HELD for status in pairs.values()):
            dependency = max(dependency, Admission.HELD)
    keep = {
        port: {
            index
            for index, key in enumerate(keys)
            if dependency is not Admission.OPEN
            or pairs[key] is Admission.OPEN
            or (include_held and pairs[key] is Admission.HELD and index in available[port])
        }
        for port, keys in members.items()
    }
    commands = [run.model_copy(update={"command": _command(job, run.command, keep)}) for run in job.commands]
    verify = [_command(job, command, keep) for command in job.verify]
    if dependency is not Admission.OPEN:
        return dependency
    inputs = {
        name: (
            artifacts
            if port not in keep
            else [artifact for index, artifact in enumerate(artifacts) if index in keep[port]]
        )
        for port, (name, artifacts) in enumerate(job.inputs.items())
    }
    paths = {path for artifacts in inputs.values() for artifact in artifacts for path in makers.paths_of(artifact)}
    checks = [
        check
        for check in job.checks
        if check.when == "after" or any(artifact.path == check.path for artifact in inputs.get(check.port, ()))
    ]
    return job.model_copy(
        update={
            "inputs": inputs,
            "missing": [
                gap
                for gap in job.missing
                if gap.member is None or pairs.get(tuple(gap.member)) is not Admission.REJECTED
            ],
            "depends_on": sorted({producers[path] for path in paths if path in producers}),
            "commands": commands,
            "verify": verify,
            "checks": checks,
        }
    )
