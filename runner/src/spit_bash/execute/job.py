from __future__ import annotations

import asyncio
import os
import shutil
import time
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Awaitable, Callable, Protocol, Union

from ..files import FolderEntries, Stamps, folder_entries, folder_inputs, stamp, stamps, stored
from ..logs import StampedLog, open_log
from ..model import Check, Dag, DagError, Job, Kind, Layout, Side
from ..reuse.relocate import Migration
from .failure import (
    CalculationFailed,
    CheckArtifactMissing,
    CheckFailed,
    CommandFailed,
    Failure,
    InputChanged,
    InputDisappeared,
    JobFilesFailed,
    ListNotWritten,
    NoCommand,
    OutputsMissing,
    Phase,
    PlaceFailed,
    ScalarInvalid,
    SeedNotCopied,
    StdoutNotMoved,
)
from ..runs import kept_path
from .. import lists, scalars
from . import seed
from .options import Options
from .process import CommandEnd, partial, run_command, succeeded

Log = Union[IO[bytes], StampedLog]


class Runner(Protocol):
    def __call__(
        self,
        dag: Dag,
        args: tuple[str, ...],
        log: Log,
        stdin: str | None = None,
        stdout: str | None = None,
        limit: int | None = None,
    ) -> Awaitable[CommandEnd]: ...


@dataclass(frozen=True)
class Made:
    inputs: Stamps
    outputs: Stamps
    seconds: float
    replaced: tuple[tuple[str, Path, Path], ...] = ()


@dataclass(frozen=True)
class Failed:
    failure: Failure
    log: Path | None


Outcome = Union[Made, Failed]

REASON_PREFIX = "spit-bash: planned to run: "


async def _checks(dag: Dag, checks: tuple[Check, ...], log: Log, runner: Runner) -> Failure | None:
    for check in checks:
        if check.side is Side.BEFORE and stamps(dag, (check.path,))[check.path] is None:
            return CheckArtifactMissing(check)
        end = await runner(dag, check.command, log)
        if not succeeded(end):
            return CheckFailed(check, end)
    return None


async def check_job(dag: Dag, job: Job, log: Path, options: Options, runner: Runner = run_command) -> Failure | None:
    with open_log(log, options.timestamp_logs) as stream:
        return await _checks(dag, job.checks, stream, runner)


def place_job(dag: Dag, placement: Migration) -> PlaceFailed | None:
    assert placement.record is not None, "a placement with moves has a record"
    done: list[tuple[str, str]] = []
    try:
        for old, new in placement.moves:
            source, target = dag.checked(old), dag.checked(new)
            if os.path.lexists(target):
                raise OSError(f"{new} already exists")
            target.parent.mkdir(parents=True, exist_ok=True)
            os.rename(source, target)
            done.append((old, new))
            key = placement.job.identities[new]
            if stored(stamp(target, new in dag.folders)) != placement.record.outputs.get(key):
                raise OSError(f"{new} does not match the record after the move")
    except (OSError, DagError) as exc:
        try:
            for old, new in reversed(done):
                os.rename(dag.checked(new), dag.checked(old))
        except (OSError, DagError) as undo:
            return PlaceFailed(str(undo), False)
        return PlaceFailed(str(exc), True)
    return None


def _clear(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def _remove_outputs(dag: Dag, job: Job, log: Log, keep: bool = False) -> None:
    for output in job.output_paths:
        try:
            path = dag.checked(output)
            if not (path.is_symlink() or path.exists()):
                continue
            if keep:
                log.write(f"spit-bash: kept failed output {output}\n".encode())
                continue
            _clear(path)
        except (OSError, DagError) as exc:
            log.write(f"spit-bash: could not remove failed output {output}: {exc}\n".encode())
        else:
            log.write(f"spit-bash: removed failed output {output}\n".encode())
    if job.stdout is not None:
        try:
            temporary = partial(dag, job.stdout)
            if not keep:
                temporary.unlink(missing_ok=True)
            elif temporary.exists():
                log.write(
                    f"spit-bash: kept failed standard output {temporary.relative_to(dag.base(job.stdout))}\n".encode()
                )
        except (OSError, DagError) as exc:
            log.write(f"spit-bash: could not handle partial output for {job.stdout}: {exc}\n".encode())
    log.flush()


def _set_aside(dag: Dag, job: Job, moved: list[tuple[str, Path, Path]]) -> None:
    for output in job.output_paths:
        path = dag.checked(output)
        if not (path.is_symlink() or path.exists()):
            continue
        waiting = Layout.previous(path)
        if waiting.is_symlink() or waiting.exists():
            _clear(path)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.rename(waiting)
        moved.append((output, path, waiting))


Listings = dict[Path, frozenset[str]]


def _listings(dag: Dag, job: Job) -> Listings:
    listed = {}
    for output in job.output_paths:
        folder = dag.checked(output).parent
        if folder not in listed:
            try:
                listed[folder] = frozenset(os.listdir(folder))
            except OSError:
                listed[folder] = frozenset()
    return listed


def _written(dag: Dag, missing: tuple[str, ...], before: Listings) -> tuple[str, ...]:
    found = set()
    for output in missing:
        folder = dag.checked(output).parent
        base = dag.base(output)
        try:
            now = os.listdir(folder)
        except OSError:
            continue
        for name in set(now) - before.get(folder, frozenset()):
            if name.endswith((".spit-bash-previous", ".spit-bash-partial")):
                continue
            relative = (folder / name).relative_to(base).as_posix()
            if relative not in dag.absolute:
                found.add(relative)
    return tuple(sorted(found))


def _settle_aside(
    moved: list[tuple[str, Path, Path]],
    log: Log,
    restore: bool,
    kept: str | None = None,
    store: Callable[[str], Path] | None = None,
) -> tuple[tuple[str, Path, Path], ...]:
    stored_at = []
    for output, path, waiting in moved:
        try:
            if restore:
                if path.is_symlink() or path.exists():
                    raise FileExistsError(f"{path} is occupied")
                waiting.rename(path)
                log.write(f"spit-bash: restored the earlier {output}\n".encode())
            elif kept is not None:
                edited = Layout.edited(path, kept)
                waiting.rename(edited)
                log.write(f"spit-bash: kept the edited {output} as {edited.name}\n".encode())
            elif store is not None:
                destination = store(output)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(waiting, destination)
                stored_at.append((output, path, destination))
                log.write(f"spit-bash: kept the replaced {output} at {destination}\n".encode())
            else:
                _clear(waiting)
        except OSError as exc:
            log.write(f"spit-bash: could not settle the earlier {output} at {waiting}: {exc}\n".encode())
    log.flush()
    return tuple(stored_at)


def _scalar_failure(job: Job, path: str, words: str) -> Failure:
    if any(run.calculation is not None and path in run.calculation.paths.values() for run in job.commands):
        return CalculationFailed(f"{path} {words}")
    return ScalarInvalid(path, words)


async def _prepare(
    dag: Dag,
    job: Job,
    log: Log,
    limit: int | None,
    runner: Runner,
    files: dict[lists.ListRead, str],
) -> Failure | dict[str, str]:
    failure = await _checks(dag, job.before, log, runner)
    if failure is not None:
        return failure
    values = scalars.read_all(job.reads, dag.path)
    if isinstance(values, tuple):
        return _scalar_failure(job, *values)
    for path, value in values.items():
        log.write(f"spit-bash: {path} holds {value}\n".encode())
    for index, command in enumerate(job.verify, 1):
        end = await runner(dag, scalars.bound(command, values, files), log, limit=limit)
        if not succeeded(end):
            return CommandFailed(Phase.VERIFY, index, end)
    return values


def _seed_outputs(dag: Dag, job: Job) -> SeedNotCopied | None:
    seeds = dict(job.seeds)
    for output in job.output_artifacts:
        written = dag.checked(output.path)
        if output.path in seeds:
            words = seed.copy_folder(dag.path(seeds[output.path]), written)
            if words is not None:
                return SeedNotCopied(output.path, words)
        elif output.kind is Kind.FOLDER:
            written.mkdir(parents=True)
        else:
            written.parent.mkdir(parents=True, exist_ok=True)
    return None


async def _run_commands(
    dag: Dag,
    job: Job,
    log: Log,
    limit: int | None,
    runner: Runner,
    values: dict[str, str],
    files: dict[lists.ListRead, str],
) -> Failure | None:
    for index, run in enumerate(job.commands, 1):
        if run.calculation is not None:
            assert run.stdout is not None, "loading gives a calculation one file output"
            log.write(f"spit-bash: calculate {run.arguments[1]} > {run.stdout}\n".encode())
            try:
                result = run.calculation.result(values)
                partial(dag, run.stdout).write_text(result + "\n", encoding="ascii")
            except (ValueError, OSError) as exc:
                return CalculationFailed(str(exc))
        else:
            end = await runner(dag, scalars.bound(run.arguments, values, files), log, run.stdin, run.stdout, limit)
            if not succeeded(end):
                return CommandFailed(Phase.COMMAND, index, end)
        if run.stdout is not None:
            try:
                partial(dag, run.stdout).replace(dag.checked(run.stdout))
            except OSError as exc:
                return StdoutNotMoved(run.stdout, str(exc))
    return None


async def _confirm(
    dag: Dag,
    job: Job,
    before: Stamps,
    entries: dict[str, FolderEntries],
    listed: Listings,
    log: Log,
    runner: Runner,
) -> Failure | tuple[Stamps, Stamps]:
    after = stamps(dag, job.input_paths)
    changed_folders = tuple(
        sorted(path for path, listed in entries.items() if listed != folder_entries(dag.path(path)))
    )
    if after != before or changed_folders:
        return InputChanged(before, after, changed_folders)
    outputs = stamps(dag, job.output_paths)
    missing = tuple(
        (port.name, artifact.path)
        for port in job.outputs
        for artifact in port.artifacts
        if outputs[artifact.path] is None
    )
    if missing:
        return OutputsMissing(missing, _written(dag, tuple(path for _, path in missing), listed))
    failure = await _checks(dag, job.after, log, runner)
    if failure is not None:
        return failure
    return after, outputs


def _prepare_outputs(dag: Dag, job: Job) -> Listings | SeedNotCopied | JobFilesFailed:
    try:
        failure = _seed_outputs(dag, job)
        return _listings(dag, job) if failure is None else failure
    except (OSError, DagError) as exc:
        return JobFilesFailed(str(exc))


async def _produce(
    dag: Dag,
    job: Job,
    before: Stamps,
    entries: dict[str, FolderEntries],
    log: Log,
    limit: int | None,
    runner: Runner,
    values: dict[str, str],
    files: dict[lists.ListRead, str],
    listed: Listings,
) -> Failure | tuple[Stamps, Stamps]:
    failure = await _run_commands(dag, job, log, limit, runner, values, files)
    if failure is not None:
        return failure
    return await _confirm(dag, job, before, entries, listed, log, runner)


def _discard(
    dag: Dag,
    job: Job,
    log: Log,
    moved: list[tuple[str, Path, Path]],
    keep: bool,
    on_discard: Callable[[], None],
) -> None:
    _remove_outputs(dag, job, log, keep)
    _settle_aside(moved, log, not keep)
    if keep:
        on_discard()


async def run_job(
    dag: Dag,
    job: Job,
    log: Path,
    options: Options,
    runner: Runner = run_command,
    on_start: Callable[[], None] = lambda: None,
    on_discard: Callable[[], None] = lambda: None,
    reason: str | None = None,
    keep_edited: bool = False,
) -> Outcome:
    before = stamps(dag, job.input_paths)
    if any(value is None for value in before.values()):
        return Failed(InputDisappeared(), None)
    if not job.commands:
        return Failed(NoCommand(), None)
    entries = folder_inputs(dag, job)
    on_start()
    started = time.monotonic()
    limit = job.memory if options.enforce_memory else None
    keep = options.keep_failed
    with open_log(log, options.timestamp_logs) as stream, lists.written(job.lists, dag.path) as files:
        if isinstance(files, str):
            return Failed(ListNotWritten(files), log)
        if reason is not None:
            stream.write(f"{REASON_PREFIX}{reason}\n".encode())
        values = await _prepare(dag, job, stream, limit, runner, files)
        if not isinstance(values, dict):
            return Failed(values, log)
        moved: list[tuple[str, Path, Path]] = []
        try:
            _set_aside(dag, job, moved)
        except (OSError, DagError) as exc:
            _settle_aside(moved, stream, True)
            return Failed(JobFilesFailed(str(exc)), log)
        listed = _prepare_outputs(dag, job)
        if not isinstance(listed, dict):
            _discard(dag, job, stream, moved, False, on_discard)
            return Failed(listed, log)
        try:
            made = await _produce(dag, job, before, entries, stream, limit, runner, values, files, listed)
        except (OSError, DagError) as exc:
            made = JobFilesFailed(str(exc))
        except asyncio.CancelledError:
            _discard(dag, job, stream, moved, keep, on_discard)
            raise
        if not isinstance(made, tuple):
            _discard(dag, job, stream, moved, keep, on_discard)
            return Failed(made, log)
        key = job.work.key
        store = (lambda output: kept_path(options.run, key, output)) if options.keep_replaced else None
        replaced = _settle_aside(moved, stream, False, options.started if keep_edited else None, store)
    after, outputs = made
    return Made(after, outputs, time.monotonic() - started, replaced)
