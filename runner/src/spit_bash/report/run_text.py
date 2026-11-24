from __future__ import annotations

import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from ..display import id_width, labels, name
from ..execute import events
from ..execute.events import Event
from ..execute.failure import (Blocked, CalculationFailed, CheckArtifactMissing, CheckFailed, CommandFailed, DependencyFailed, Failure,
                               InputChanged, InputDisappeared, NoCommand, OutputsMissing, Phase, PlaceFailed,
                               InputNotVisible, ListNotWritten, ScalarInvalid, ScreenFailed, SeedNotCopied, SlurmEnded,
                               StdoutNotMoved)
from ..execute.process import CommandEnd, Exited, KilledMemory, KilledSignal, NotStarted
from ..execute.scheduler import Result
from ..files import stored
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


def end_words(kind: str, end: CommandEnd) -> str:
    match end:
        case Exited(code):
            return f"{kind} exited with status {code}"
        case KilledMemory(used, limit):
            return f"{kind} used {size(used)} of memory, more than the {size(limit)} its job declares, and was killed"
        case KilledSignal(limit):
            declared = f", declared memory={size(limit)}" if limit is not None else ""
            return f"{kind} killed by signal 9 (SIGKILL), possibly out of memory{declared}"
        case NotStarted(error):
            return f"{kind} could not start: {error}"


def _input_changes(failure: InputChanged) -> str:
    before, after = failure.before, failure.after
    message = "input changed during execution"
    if after != before:
        changes = []
        for path, original in before.items():
            current = after.get(path)
            if original == current:
                continue
            if original is None or current is None:
                changes.append(f"{path}: {stored(original)!r} -> {stored(current)!r}")
                continue
            changes.append(f"{path}: {', '.join(original.changes(current))}")
        message += ": " + "; ".join(changes)
    extra = [f"{path}: directory entries or file metadata changed" for path in failure.folders
             if before[path] == after[path]]
    return message + ("; " + "; ".join(extra) if extra else "")


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
        case CommandFailed(Phase.VERIFY, index, end):
            return end_words(f"verify {index}", end)
        case CommandFailed(Phase.COMMAND, index, end):
            return end_words(job.commands[index - 1].describe(index, len(job.commands)), end)
        case ListNotWritten(words):
            return words
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
        case OutputsMissing(missing, written):
            words = "command succeeded but did not create " + ", ".join(f"output {port} {path}" for port, path in missing)
            if written:
                shown = ", ".join(written[:WRITTEN_SHOWN])
                more = f" and {len(written) - WRITTEN_SHOWN} more" if len(written) > WRITTEN_SHOWN else ""
                words += f"; it wrote {shown}{more} in the same folder, left in place"
            return words
        case PlaceFailed(error, True):
            return f"could not move its outputs, so they were put back: {error}"
        case PlaceFailed(error, False):
            return f"could not move its outputs, and putting them back failed: {error}"
        case InputNotVisible(unseen, seconds):
            shown = "; ".join(f"{path}: recorded {list(wanted)}, seen {'nothing' if seen is None else list(seen)}"
                              for path, wanted, seen in unseen)
            return (f"inputs another node wrote did not show here as their jobs recorded them within {seconds:.0f}s: "
                    f"{shown}")
        case SlurmEnded(state, words, worker_failure):
            heading = f"Slurm ended its task{'' if state is None else f' as {state}'}"
            detail = failure_words(job, worker_failure) if worker_failure is not None else words
            return f"{heading}: {detail}" if detail else heading
        case DependencyFailed(first):
            return f"dependency {first} failed"
        case Blocked(decision):
            return cause(decision.cause)
    raise AssertionError(f"no words for {failure!r}")


def printer(dag: Dag) -> Callable[[Event], None]:
    work, width = labels(dag.jobs), id_width(dag.jobs)

    def say(job: Job, word: str, rest: str = "") -> None:
        print(f"  [{job.id:>{width}}] {word:7} {name(job, work[job.id])}{rest}", flush=True)

    def on_event(event: Event) -> None:
        match event:
            case events.RunsAlone(job, over):
                print(f"[{job.id}] needs {over}; it will run alone, once no other job is running", flush=True)
            case events.Running(0, 0, _, places):
                print(f"\nMoving the outputs of {places} job{'' if places == 1 else 's'}", flush=True)
            case events.Running(runs, maybe, remaining, places):
                estimate = "" if remaining is None else f", {about(remaining)}"
                more = f", and {maybe} more that may" if maybe else ""
                moving = f", and moving the outputs of {places}" if places else ""
                print(f"\nRunning {runs} job{'' if runs == 1 else 's'}{more}{estimate}{moving}", flush=True)
            case events.Queued(job, task):
                say(job, "queued", f": Slurm task {task}")
            case events.Retrying(job, attempt, why, resources):
                say(job, "retry", f": Slurm ended it {why}; attempt {attempt} with {resources}")
            case events.Started(job):
                say(job, "start")
            case events.Done(job, seconds, progress):
                say(job, "done", f" in {duration(seconds)}; {progress}")
            case events.Placed(job, moves):
                say(job, "moved", ": " + ", ".join(f"{old} -> {new}" for old, new in moves))
            case events.Checking(job):
                say(job, "check")
            case events.Checked(job):
                say(job, "checked")
            case events.SkippedUnchanged(job):
                say(job, "skip", ": upstream output unchanged")
            case events.ScreenNote(text):
                print(text, file=sys.stderr)
            case events.JobFailed(result):
                assert result.failure is not None, "a failed job has its failure"
                say(result.job, "failed", ": " + failure_words(result.job, result.failure))
            case events.NotRun():
                pass

    return on_event


@dataclass(frozen=True)
class Shown:
    job: Job
    error: str
    log: Path | None


def _shown(result: Result) -> Shown:
    assert result.failure is not None, "only a failed job is shown as one"
    return Shown(result.job, failure_words(result.job, result.failure), result.log)


def _report_failure(job: Job, error: str, log: Path | None) -> None:
    job_id = job.id
    print(f"[{job_id}] failed: {error}", file=sys.stderr)
    for line in job.origin:
        print(f"[{job_id}] {line}", file=sys.stderr)
    if log is not None:
        tail = log_tail(log)
        if tail:
            print(f"[{job_id}] last lines of {log}:", file=sys.stderr)
            for line in tail:
                print(f"    {line}", file=sys.stderr)
        else:
            print(f"[{job_id}] log: {log}", file=sys.stderr)


def report_failures(dag: Dag, summary: Summary, verbose: bool) -> None:
    held = Counter(result.caused_by for result in summary.not_run)
    for group in failure_groups(dag, [_shown(result) for result in summary.failed]):
        first, others = group[0], group[1:]
        _report_failure(first.job, first.error, first.log)
        if others:
            print(f"[{first.job.id}] the same failure in {len(others)} other job{'' if len(others) == 1 else 's'}: {ids(others)}", file=sys.stderr)
        total = sum(held[shown.job.id] for shown in group)
        if total:
            lead = "together they held up" if others else "held up"
            print(f"[{first.job.id}] {lead} {total} other job{'' if total == 1 else 's'}", file=sys.stderr)
    if verbose:
        for result in summary.not_run:
            print(f"[{result.job.id}] not run: job {result.caused_by} failed", file=sys.stderr)


def summary_line(summary: Summary) -> str:
    not_run = len(summary.not_run)
    line = (
        f"{summary.done} done, "
        + (f"{summary.placed} moved, " if summary.placed else "")
        + f"{summary.checked} checked, {len(summary.failed)} failed, "
        + (f"{not_run} not run (an upstream job failed), " if not_run else "")
        + f"{summary.current} current, "
        + (f"{summary.kept} kept (upstream reran with unchanged output), " if summary.kept else "")
        + f"{summary.blocked} blocked"
    )
    return line


def varied_notes(results: tuple[Result, ...]) -> list[str]:
    by_operation = Counter(result.job.operation for result in results if result.varied)
    return [f"note: {operation} gave different output from the same inputs ({count} job{'' if count == 1 else 's'}); "
            "what reads it will rerun every time"
            for operation, count in sorted(by_operation.items())]


def moved_note() -> str:
    return ("outputs were moved to the paths the pipeline now gives them; a tool that wrote those paths into "
            "its output is not rerun")


def left_lines(left: Left) -> list[str]:
    if left.stale:
        return [f"{left.stale} job{'' if left.stale == 1 else 's'} outside the selection read outputs this run changed; "
                "run without the selection to bring them up to date"]
    if left.unchanged:
        were = "output was" if left.unchanged == 1 else "outputs were"
        return [f"{left.unchanged} {were} rewritten with the same content; nothing outside the selection needs to run"]
    return []


def counts(summary: Summary) -> dict[str, int]:
    return {"done": summary.done, "moved": summary.placed, "checked": summary.checked, "failed": len(summary.failed),
            "not_run": len(summary.not_run), "current": summary.current, "kept": summary.kept,
            "blocked": summary.blocked}


def replaced_lines(run: runs.Run) -> list[str]:
    found = runs.kept(run)
    if not found:
        return []
    files = sum(len(each.files) for each in found)
    taken = size(runs.size(path for each in found for _, _, path in each.files)) + "B"
    jobs = f"{len(found)} job{'' if len(found) == 1 else 's'}"
    return [f"kept the {files} output{'' if files == 1 else 's'} that {jobs} replaced ({taken}) in "
            f"{run.folder / 'replaced'}; `spit-bash restore --run {run.name}` puts {'it' if files == 1 else 'them'} back"]


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
    lines = [f"restored {paths} output{'' if paths == 1 else 's'} of {len(restored)} "
             f"job{'' if len(restored) == 1 else 's'}, with the records they had"]
    aside = sum(len(each.kept) for each in restored)
    if aside:
        lines.append(f"kept the {aside} output{'' if aside == 1 else 's'} they replaced in {undo.folder / 'replaced'}; "
                     f"`spit-bash restore --run {undo.name}` puts {'it' if aside == 1 else 'them'} back")
    return lines
