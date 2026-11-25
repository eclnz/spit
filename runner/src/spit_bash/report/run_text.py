from __future__ import annotations

import shlex
import sys
from collections import Counter
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Callable

from ..display import id_width, labels, name
from ..execute import events
from ..execute.events import Event
from ..execute.failure import (
    Blocked,
    CalculationFailed,
    CheckArtifactMissing,
    CheckFailed,
    CommandFailed,
    DependencyFailed,
    Failure,
    InputChanged,
    InputDisappeared,
    JobFilesFailed,
    NoCommand,
    OutputsMissing,
    Phase,
    PlaceFailed,
    InputNotVisible,
    ListNotWritten,
    ScalarInvalid,
    ScreenFailed,
    SeedNotCopied,
    SlurmEnded,
    StdoutNotMoved,
)
from ..execute.process import CommandEnd, Exited, KilledMemory, KilledSignal, NotStarted
from ..execute.scheduler import Result
from ..files import Stamp, Stamps, stored
from ..logs import log_tail
from .. import runs
from ..model import Dag, Job
from ..state.record import Record
from ..progress import about, duration
from ..schedule import size
from .causes import cause
from .failures import failure_groups, ids
from .summary import Left, Summary


WRITTEN_SHOWN = 5


class Word(Enum):
    QUEUED = "queued"
    RETRY = "retry"
    START = "start"
    DONE = "done"
    MOVED = "moved"
    CHECK = "check"
    CHECKED = "checked"
    SKIP = "skip"
    FAILED = "failed"


def _count(number: int, noun: str) -> str:
    return f"{number} {noun}{'' if number == 1 else 's'}"


def _them(number: int) -> str:
    return "it" if number == 1 else "them"


def _declared(limit: int | None) -> str:
    return "" if limit is None else f", declared memory={size(limit)}"


def end_words(kind: str, end: CommandEnd) -> str:
    match end:
        case Exited(code):
            return f"{kind} exited with status {code}"
        case KilledMemory(used, limit):
            return f"{kind} used {size(used)} of memory, more than the {size(limit)} its job declares, and was killed"
        case KilledSignal(limit):
            return f"{kind} killed by signal 9 (SIGKILL), possibly out of memory{_declared(limit)}"
        case NotStarted(error):
            return f"{kind} could not start: {error}"


def _path_change(path: str, original: Stamp | None, current: Stamp | None) -> str:
    if original is None or current is None:
        return f"{path}: {stored(original)!r} -> {stored(current)!r}"
    return f"{path}: {', '.join(original.changes(current))}"


def _stamp_changes(before: Stamps, after: Stamps) -> str:
    if after == before:
        return ""
    changes = [
        _path_change(path, original, after.get(path))
        for path, original in before.items()
        if original != after.get(path)
    ]
    return ": " + "; ".join(changes)


def _folder_changes(failure: InputChanged) -> str:
    extra = [
        f"{path}: directory entries or file metadata changed"
        for path in failure.folders
        if failure.before[path] == failure.after[path]
    ]
    return "; " + "; ".join(extra) if extra else ""


def _input_changes(failure: InputChanged) -> str:
    return "input changed during execution" + _stamp_changes(failure.before, failure.after) + _folder_changes(failure)


def _command_kind(job: Job, phase: Phase, index: int) -> str:
    match phase:
        case Phase.VERIFY:
            return f"verify {index}"
        case Phase.COMMAND:
            return job.commands[index - 1].describe(index, len(job.commands))


def _written_words(written: tuple[str, ...]) -> str:
    if not written:
        return ""
    shown = ", ".join(written[:WRITTEN_SHOWN])
    more = f" and {len(written) - WRITTEN_SHOWN} more" if len(written) > WRITTEN_SHOWN else ""
    return f"; it wrote {shown}{more} in the same folder, left in place"


def _outputs_missing(failure: OutputsMissing) -> str:
    missing = ", ".join(f"output {port} {path}" for port, path in failure.missing)
    return f"command succeeded but did not create {missing}{_written_words(failure.written)}"


def _not_visible(failure: InputNotVisible) -> str:
    shown = "; ".join(
        f"{path}: recorded {list(wanted)}, seen {'nothing' if seen is None else list(seen)}"
        for path, wanted, seen in failure.unseen
    )
    return (
        f"inputs another node wrote did not show here as their jobs recorded them within {failure.seconds:.0f}s: "
        f"{shown}"
    )


def _slurm_ended(job: Job, failure: SlurmEnded) -> str:
    heading = f"Slurm ended its task{'' if failure.state is None else f' as {failure.state}'}"
    detail = failure.words if failure.worker_failure is None else failure_words(job, failure.worker_failure)
    return f"{heading}: {detail}" if detail else heading


def failure_words(job: Job, failure: Failure) -> str:
    match failure:
        case InputDisappeared():
            return "input disappeared before execution"
        case NoCommand():
            return "job has no command"
        case CheckArtifactMissing(check):
            return f"{check.describe()}: the artifact is missing"
        case ScreenFailed(screen, path, reason):
            return f"screen {screen} on {path}: {reason}"
        case CheckFailed(check, end):
            return end_words(check.describe(), end).replace(" exited ", " failed: exited ", 1)
        case CommandFailed(phase, index, end):
            return end_words(_command_kind(job, phase, index), end)
        case ListNotWritten(words):
            return words
        case JobFilesFailed(error):
            return f"could not prepare or check the job's files: {error}"
        case SeedNotCopied(path, words):
            return f"could not start {path} as a copy of its input folder: {words}"
        case CalculationFailed(words):
            return f"calculation failed: {words}"
        case ScalarInvalid(path, words):
            return f"{path}, read by `@scalar`, {words}"
        case StdoutNotMoved(path, error):
            return f"could not move standard output onto {path}: {error}"
        case InputChanged():
            return _input_changes(failure)
        case OutputsMissing():
            return _outputs_missing(failure)
        case PlaceFailed(error, True):
            return f"could not move its outputs, so they were put back: {error}"
        case PlaceFailed(error, False):
            return f"could not move its outputs, and putting them back failed: {error}"
        case InputNotVisible():
            return _not_visible(failure)
        case SlurmEnded():
            return _slurm_ended(job, failure)
        case DependencyFailed(first):
            return f"dependency {first} failed"
        case Blocked(decision):
            return cause(decision.cause)
    raise AssertionError(f"no words for {failure!r}")


def _result_words(result: Result) -> str:
    assert result.failure is not None, "a failed job has its failure"
    return failure_words(result.job, result.failure)


def _moving_line(places: int) -> str:
    return f"\nMoving the outputs of {_count(places, 'job')}"


def _running_line(event: events.Running) -> str:
    more = f", and {event.maybe} more that may" if event.maybe else ""
    estimate = "" if event.remaining is None else f", {about(event.remaining)}"
    moving = f", and moving the outputs of {event.places}" if event.places else ""
    return f"\nRunning {_count(event.runs, 'job')}{more}{estimate}{moving}"


def _moves(moves: tuple[tuple[str, str], ...]) -> str:
    return ", ".join(f"{old} -> {new}" for old, new in moves)


def printer(dag: Dag) -> Callable[[Event], None]:
    work, width = labels(dag.jobs), id_width(dag.jobs)

    def say(job: Job, word: Word, rest: str = "") -> None:
        print(f"  [{job.id:>{width}}] {word.value:7} {name(job, work[job.id])}{rest}", flush=True)

    def on_event(event: Event) -> None:
        match event:
            case events.RunsAlone(job, over):
                print(f"[{job.id}] needs {over}; it will run alone, once no other job is running", flush=True)
            case events.Running(0, 0, _, places):
                print(_moving_line(places), flush=True)
            case events.Running():
                print(_running_line(event), flush=True)
            case events.Queued(job, task):
                say(job, Word.QUEUED, f": Slurm task {task}")
            case events.Retrying(job, attempt, why, resources):
                say(job, Word.RETRY, f": Slurm ended it {why}; attempt {attempt} with {resources}")
            case events.Started(job):
                say(job, Word.START)
            case events.Done(job, seconds, progress):
                say(job, Word.DONE, f" in {duration(seconds)}; {progress}")
            case events.Placed(job, moves):
                say(job, Word.MOVED, ": " + _moves(moves))
            case events.Checking(job):
                say(job, Word.CHECK)
            case events.Checked(job):
                say(job, Word.CHECKED)
            case events.SkippedUnchanged(job):
                say(job, Word.SKIP, ": upstream output unchanged")
            case events.ScreenNote(text):
                print(text, file=sys.stderr)
            case events.JobFailed(result):
                say(result.job, Word.FAILED, ": " + _result_words(result))
            case events.NotRun():
                pass

    return on_event


@dataclass(frozen=True)
class Shown:
    job: Job
    error: str
    log: Path | None


def _shown(result: Result) -> Shown:
    return Shown(result.job, _result_words(result), result.log)


def _log_lines(job_id: int, log: Path) -> list[str]:
    tail = log_tail(log)
    if not tail:
        return [f"[{job_id}] log: {log}"]
    return [f"[{job_id}] last lines of {log}:", *(f"    {line}" for line in tail)]


def _failure_lines(shown: Shown) -> list[str]:
    job_id = shown.job.id
    lines = [f"[{job_id}] failed: {shown.error}", *(f"[{job_id}] {line}" for line in shown.job.origin)]
    return lines if shown.log is None else lines + _log_lines(job_id, shown.log)


def _group_lines(group: list[Shown], held: Counter[int | None]) -> list[str]:
    first, others = group[0], group[1:]
    lines = _failure_lines(first)
    if others:
        lines.append(f"[{first.job.id}] the same failure in {_count(len(others), 'other job')}: {ids(others)}")
    total = sum(held[shown.job.id] for shown in group)
    if total:
        lead = "together they held up" if others else "held up"
        lines.append(f"[{first.job.id}] {lead} {_count(total, 'other job')}")
    return lines


def report_failures(dag: Dag, summary: Summary, verbose: bool) -> None:
    held = Counter(result.caused_by for result in summary.not_run)
    for group in failure_groups(dag, [_shown(result) for result in summary.failed]):
        for line in _group_lines(group, held):
            print(line, file=sys.stderr)
    if verbose:
        for result in summary.not_run:
            print(f"[{result.job.id}] not run: job {result.caused_by} failed", file=sys.stderr)


def summary_line(summary: Summary, held: int = 0, rejected: int = 0) -> str:
    not_run = len(summary.not_run)
    line = (
        f"{summary.done} done, "
        + (f"{summary.placed} moved, " if summary.placed else "")
        + f"{summary.checked} checked, {len(summary.failed)} failed, "
        + (f"{not_run} not run (an upstream job failed), " if not_run else "")
        + (f"{held} held for review, " if held else "")
        + (f"{rejected} not required, " if rejected else "")
        + f"{summary.current} current, "
        + (f"{summary.kept} kept (upstream reran with unchanged output), " if summary.kept else "")
        + f"{summary.blocked} blocked"
    )
    return line


def varied_notes(results: tuple[Result, ...]) -> list[str]:
    by_operation = Counter(result.job.operation for result in results if result.varied)
    return [
        f"note: {operation} gave different output from the same inputs ({_count(count, 'job')}); "
        "what reads it will rerun every time"
        for operation, count in sorted(by_operation.items())
    ]


def moved_note() -> str:
    return (
        "outputs were moved to the paths the pipeline now gives them; a tool that wrote those paths into "
        "its output is not rerun"
    )


def left_lines(left: Left) -> list[str]:
    if left.stale:
        return [
            f"{_count(left.stale, 'job')} outside the selection read outputs this run changed; "
            "run without the selection to bring them up to date"
        ]
    if left.unchanged:
        were = "output was" if left.unchanged == 1 else "outputs were"
        return [f"{left.unchanged} {were} rewritten with the same content; nothing outside the selection needs to run"]
    return []


def counts(summary: Summary) -> dict[str, int]:
    return {
        "done": summary.done,
        "moved": summary.placed,
        "checked": summary.checked,
        "failed": len(summary.failed),
        "not_run": len(summary.not_run),
        "current": summary.current,
        "kept": summary.kept,
        "blocked": summary.blocked,
    }


def _kept_line(files: int, replaced: str, run: runs.Run) -> str:
    return (
        f"kept the {_count(files, 'output')} {replaced} in {run.folder / 'replaced'}; "
        f"`spit-bash restore --dir {shlex.quote(str(run.folder.parent.parent))} --run {run.name}` "
        f"puts {_them(files)} back"
    )


def replaced_lines(run: runs.Run) -> list[str]:
    found = runs.kept(run)
    if not found:
        return []
    files = sum(len(each.files) for each in found)
    taken = size(runs.size(path for each in found for _, _, path in each.files)) + "B"
    return [_kept_line(files, f"that {_count(len(found), 'job')} replaced ({taken})", run)]


def log_lines(dag: Dag, jobs: list[Job], history: list[runs.Run], records: dict[str, Record]) -> list[str]:
    named = labels(dag.jobs)
    width = id_width(jobs)
    lines = []
    for job in jobs:
        key = job.work.key
        record = records.get(key)
        made = None if record is None else record.run
        lines.append(f"[{job.id:>{width}}] {name(job, named[job.id])}")
        found = [run for run in history if run.job_log(key).is_file()]
        for run in found:
            mark = "  (made its current outputs)" if run.name == made else ""
            lines.append(f"  {run.name}  {run.job_log(key)}{mark}")
        if not found:
            lines.append("  no log kept")
    return lines


def restore_lines(restored: list[runs.Restored], undo: runs.Run) -> list[str]:
    if not restored:
        return ["nothing to restore: the run kept no outputs of the chosen jobs, or they were restored already"]
    paths = sum(len(each.paths) for each in restored)
    lines = [f"restored {_count(paths, 'output')} of {_count(len(restored), 'job')}, with the records they had"]
    aside = sum(len(each.kept) for each in restored)
    return lines + [_kept_line(aside, "they replaced", undo)] if aside else lines
