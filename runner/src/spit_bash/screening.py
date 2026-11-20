"""Bound screens and their verdicts, applied after artifact checks and before job reuse."""

from __future__ import annotations

import asyncio
import csv
import hashlib
import json
import os
import shutil
from dataclasses import dataclass, field
from pathlib import Path
from tempfile import NamedTemporaryFile, TemporaryFile
from typing import Callable, TextIO

from . import schema
from .execute.failure import Failure, ScreenFailed
from .execute.process import Exited, NotStarted, run_command
from .files import content_hash
from .gates import Admission, Producers, ReviewDecisions, Scope, Verdict, select_job
from .model import Dag, DagError, Job

# Changing the evidence layout invalidates verdicts recorded with older dependency rules.
EVIDENCE_VERSION = 2


@dataclass(frozen=True)
class BoundScreen:
    scope: Scope
    port: str
    label: str
    command: tuple[str, ...]
    artifact: schema.Artifact
    record: str


@dataclass(frozen=True)
class ScreenPlan:
    jobs: dict[int, schema.SpitJob]
    before: dict[int, list[BoundScreen]]
    after: dict[int, list[BoundScreen]]
    reviewed: tuple[BoundScreen, ...]
    affected: frozenset[int]
    reviews: ReviewDecisions
    excluded: dict[int, Admission]
    producers: Producers
    materialize: Callable[[schema.SpitJob], Job]


def screen_attachments(document: schema.SpitDag) -> dict[int, list[tuple[Scope, schema.Screen, schema.Artifact]]]:
    found = {}
    claimed = {artifact.path.casefold() for job in document.jobs for artifact in job.outputs.values()}
    claimed.update(artifact.path.casefold() for artifact in document.external_inputs)
    for record in document.screen_records.values():
        if record.casefold() in claimed:
            raise DagError(f"screen record collides at {record}")
        claimed.add(record.casefold())
    for job in document.jobs:
        if not job.screens:
            continue
        found[job.id] = []
        for screen in job.screens:
            if screen.when == "after":
                artifact = job.outputs.get(screen.port)
                scope = Scope(screen.path)
                if artifact is not None and artifact.path != screen.path:
                    artifact = None
            else:
                artifact = next((item for item in job.inputs.get(screen.port, ()) if item.path == screen.path), None)
                index = tuple(job.inputs).index(screen.port) if artifact is not None else None
                scope = Scope(screen.path) if screen.product_scope else Scope(screen.path, job.id, index)
            if artifact is None or artifact.product not in document.screen_records:
                raise DagError(f"job {job.id} has an invalid screen target or record")
            found[job.id].append((scope, screen, artifact))
    return found


def check_record_locations(document: schema.SpitDag, dag: Dag) -> None:
    """Resolve both sides before a record can replace a review or an artifact."""
    if not document.screen_records:
        return
    claimed = {dag.checked(artifact.path).resolve()
               for job in document.jobs for artifact in job.outputs.values()}
    claimed.update(dag.checked(artifact.path).resolve() for artifact in document.external_inputs)
    claimed.update(Path(review.file).resolve() for review in document.reviews if review.file is not None)
    ancestors = {parent for path in claimed for parent in path.parents}
    for record in document.screen_records.values():
        path = dag.checked(record, "screen record").resolve()
        if path in claimed or path in ancestors or any(parent in claimed for parent in path.parents):
            raise DagError(f"screen record overlaps a review or artifact at {record}")
        claimed.add(path)
        ancestors.update(path.parents)


@dataclass(frozen=True)
class Outcome:
    verdict: Verdict
    reason: str
    evidence: str = ""


@dataclass
class ScreenRun:
    plan: ScreenPlan
    state: Path
    scopes: dict[Scope, Admission]
    excluded: dict[int, Admission]
    cached: dict[str, tuple[str, str, str, str]]
    slots: asyncio.Semaphore
    observed: dict[str, tuple[BoundScreen, Outcome]] = field(default_factory=dict)
    tasks: dict[str, asyncio.Task[Outcome | Failure]] = field(default_factory=dict)
    hashes: dict[tuple[str, bool], asyncio.Task[str | None]] = field(default_factory=dict)

    @classmethod
    def start(cls, plan: ScreenPlan, state: Path, cores: int) -> ScreenRun:
        try:
            raw = json.loads(state.read_text(encoding="utf-8"))
            cached = {key: tuple(value) for key, value in raw.items()
                      if isinstance(value, list) and len(value) == 4 and all(isinstance(x, str) for x in value)
                      and value[1] in ("pass", "fail")} if isinstance(raw, dict) else {}
        except (OSError, ValueError):
            cached = {}
        return cls(plan, state, dict(plan.reviews.scopes), dict(plan.excluded), cached, asyncio.Semaphore(cores))


def selected(run: ScreenRun, job_id: int, *, include_held: bool = False) -> schema.SpitJob | Admission:
    return select_job(run.plan.jobs[job_id], run.scopes, run.excluded, run.plan.producers, include_held=include_held)


def _key(screen: BoundScreen) -> str:
    return json.dumps((screen.record, screen.scope.path, screen.scope.job, screen.scope.port, screen.label, screen.command),
                      separators=(",", ":"))


async def evaluate(run: ScreenRun, dag: Dag, screen: BoundScreen, log: Path) -> Failure | None:
    key = _key(screen)
    if key not in run.tasks:
        run.tasks[key] = asyncio.create_task(_evaluate(run, dag, screen, log))
    outcome = await run.tasks[key]
    if not isinstance(outcome, Outcome):
        return outcome
    run.observed[key] = screen, outcome
    if outcome.verdict is Verdict.FAIL:
        run.scopes[screen.scope] = Admission.REJECTED
    return None


async def _digest(run: ScreenRun, path: Path, folder: bool = False) -> str | None:
    key = str(path), folder
    if key not in run.hashes:
        run.hashes[key] = asyncio.create_task(asyncio.to_thread(content_hash, *key))
    return await run.hashes[key]


async def _evaluate(run: ScreenRun, dag: Dag, screen: BoundScreen, log: Path) -> Outcome | Failure:
    path = dag.checked(screen.artifact.path, "screen")
    artifact_hash = await _digest(run, path, screen.artifact.kind != "file")
    if artifact_hash is None:
        return ScreenFailed(screen.label, screen.artifact.path, "artifact missing")
    program = screen.command[0]
    search_path = os.pathsep.join(str(dag.outputs / folder) for folder in os.get_exec_path())
    executable = shutil.which(program, path=search_path) if "/" not in program else str(dag.outputs / program)
    tool_hash = None if executable is None else await _digest(run, Path(executable))
    helper_hashes = []
    for word in screen.command[1:]:
        helper = dag.outputs / word
        # Interpreters take script filenames relative to the command's working directory.
        if helper != path and os.path.isfile(helper):
            helper_hashes.append((word, await _digest(run, helper)))
    evidence = hashlib.sha256(json.dumps(
        (EVIDENCE_VERSION, artifact_hash, screen.command, executable, tool_hash, helper_hashes,
         sorted((name, tool.digest) for name, tool in dag.tools.items())),
    ).encode()).hexdigest()
    record = dag.checked(screen.record, "screen record")
    cached = run.cached.get(_key(screen))
    if (tool_hash is not None and cached is not None and cached[0] == evidence
            and await _digest(run, record) == cached[3]):
        return Outcome(Verdict(cached[1]), cached[2], evidence)
    log.parent.mkdir(parents=True, exist_ok=True)
    async with run.slots:
        with log.open("ab") as output, TemporaryFile() as stderr:
            end = await run_command(dag, screen.command, output, stderr=stderr)
            stderr.seek(0)
            reason = ""
            for line in stderr:
                output.write(line)
                text = " ".join(line.decode(errors="replace").split())
                if text:
                    reason = text
    if not isinstance(end, Exited):
        return ScreenFailed(screen.label, screen.artifact.path,
                            end.error if isinstance(end, NotStarted) else "command terminated by a signal")
    if end.code < 0:
        return ScreenFailed(screen.label, screen.artifact.path, f"command terminated by signal {-end.code}")
    if await asyncio.to_thread(content_hash, str(path), screen.artifact.kind != "file") != artifact_hash:
        return ScreenFailed(screen.label, screen.artifact.path, "artifact changed during screening")
    if end.code == 0:
        return Outcome(Verdict.PASS, "", evidence if tool_hash is not None else "")
    return Outcome(Verdict.FAIL, reason or f"exit status {end.code}", evidence if tool_hash is not None else "")


def _atomic(path: Path, write: Callable[[TextIO], object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with NamedTemporaryFile(mode="w", encoding="utf-8", newline="", dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        try:
            write(file)
            file.flush()
            os.fsync(file.fileno())
            file.close()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)


def publish(run: ScreenRun, dag: Dag, failed: set[int]) -> None:
    """Write one sorted record per evaluated product, then publish its cache evidence."""
    for screen in run.plan.reviewed:
        producer = run.plan.producers.paths.get(screen.artifact.path)
        if producer in failed or producer in run.excluded or not dag.checked(screen.artifact.path).exists():
            continue
        human = run.plan.reviews.overrides[screen.scope]
        why = f"review line {human.line}: {human.verdict.value}"
        reason = f"{why}: {human.reason}" if human.reason else why
        run.observed[_key(screen)] = screen, Outcome(human.verdict, reason)
    products: dict[str, list[tuple[BoundScreen, Outcome]]] = {}
    for screen, outcome in run.observed.values():
        products.setdefault(screen.record, []).append((screen, outcome))
    digests = {}
    for record, outcomes in sorted(products.items()):
        rows = [({**screen.artifact.entities, "screen": screen.label,
                  "verdict": outcome.verdict.value if outcome.evidence else (
                      "reviewed" if screen.scope in run.plan.reviews.overrides else outcome.verdict.value),
                  "reason": outcome.reason}, screen)
                for screen, outcome in outcomes]
        dimensions = list(rows[0][1].artifact.entities)
        keys = [*dimensions, "screen", "verdict", "reason"]
        rows.sort(key=lambda entry: tuple(entry[0][key] for key in keys))

        def write(file: TextIO) -> None:
            writer = csv.DictWriter(file, keys, lineterminator="\n")
            writer.writeheader()
            writer.writerows(row for row, _ in rows)

        path = dag.checked(record, "screen record")
        _atomic(path, write)
        digests[record] = content_hash(str(path))
    cached = {key: (outcome.evidence, outcome.verdict.value, outcome.reason, digests[screen.record])
              for key, (screen, outcome) in run.observed.items() if outcome.evidence}
    if products:
        _atomic(run.state, lambda file: json.dump(cached, file, sort_keys=True))


def lines(run: ScreenRun) -> list[str]:
    return [f"screen `{screen.label}` rejected {screen.artifact.path}: {result.reason}"
            for _, (screen, result) in sorted(run.observed.items())
            if result.verdict is Verdict.FAIL and screen.scope not in run.plan.reviews.overrides]
