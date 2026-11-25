from __future__ import annotations

import json
import shlex
from dataclasses import dataclass
from pathlib import Path

from .. import runs
from ..leftovers import amount, leftovers, lines as leftover_lines
from ..model import Dag, DagError, Layout
from ..slurm import commands, site
from ..slurm.controller import folder as slurm_folder
from ..slurm.executor import job_name
from ..slurm.profile import Profile
from ..slurm.survey import Survey, survey
from ..state.record import Record
from ..state.store import Store

QUIET = (runs.Ending.RESTORED, runs.Ending.RELEASED)


@dataclass(frozen=True)
class Section:
    name: str
    attention: bool
    lines: tuple[str, ...]


def sections(layout: Layout, dag: Dag | None, failure: str | None, profile: Profile | None) -> list[Section]:
    state = _state(layout, dag)
    found = [_slurm(layout, profile), state.section, _last_run(layout), _kept(layout)]
    found.append(_leftovers(dag, failure, state.records))
    return found


def text(found: list[Section]) -> list[str]:
    lines = []
    for section in found:
        lines.append(f"{section.name}: {'needs attention' if section.attention else 'ok'}")
        lines += [f"  {line}" for line in section.lines]
    return lines


@dataclass(frozen=True)
class Finding:
    attention: bool
    lines: tuple[str, ...]


@dataclass(frozen=True)
class State:
    records: dict[str, Record] | None
    section: Section


def _count(count: int, noun: str) -> str:
    return f"{count} {noun}{'' if count == 1 else 's'}"


def _slurm(layout: Layout, profile: Profile | None) -> Section:
    found = (_reservations(layout), _site(layout, profile))
    return Section("slurm", any(each.attention for each in found), tuple(line for each in found for line in each.lines))


def _reservations(layout: Layout) -> Finding:
    slurm = slurm_folder(layout)
    if not any(slurm.glob("*.dispatch")):
        return Finding(False, ("no Slurm reservations",))
    try:
        commands.require_local_cluster()
        surveyed = survey(slurm, job_name(layout.folder), write=False)
    except DagError as exc:
        return _unread(str(exc))
    if surveyed is None:
        return _unread()
    return _queue(surveyed, layout.folder)


def _unread(*reasons: str) -> Finding:
    return Finding(True, (*reasons, "cannot read the Slurm queue with squeue, so its reservations are not checked"))


def _queue(surveyed: Survey, folder: Path) -> Finding:
    queued = len(surveyed.queued)
    lines = (_queued(queued),) if queued else ()
    if surveyed.unverified:
        return Finding(True, (*lines, *surveyed.problems(), *surveyed.advice(folder)))
    return Finding(False, lines or ("every reservation is settled",))


def _queued(count: int) -> str:
    return f"{_count(count, 'Slurm task')} of this folder {'is' if count == 1 else 'are'} queued or running"


def _site(layout: Layout, profile: Profile | None) -> Finding:
    report = site.load(layout, profile)
    match profile, report:
        case Profile() as chosen, _:
            return _verdict(report, chosen)
        case None, site.Report() as latest:
            return _latest_check(latest)
        case _:
            return Finding(False, ())


def _verdict(report: site.Report | None, profile: Profile) -> Finding:
    verdict = site.verdict(report, profile)
    if verdict is None:
        return Finding(False, ("the site check for this profile passed",))
    return Finding(verdict.blocks, (verdict.message,))


def _latest_check(report: site.Report) -> Finding:
    if not report.failed:
        return Finding(False, ("the latest site check passed",))
    listing = "; ".join(f"{check.name.value}: {check.detail}" for check in report.failed)
    return Finding(True, (f"the latest site check failed: {listing}; `spit-bash check-site` again once it is fixed",))


def _state(layout: Layout, dag: Dag | None) -> State:
    path = layout.state
    if not path.exists():
        return State({}, _unrecorded(path.name, _made(dag)))
    try:
        store = Store(path)
    except DagError as exc:
        return State(None, Section("state", True, (str(exc),)))
    records = _records(store)
    return State(records, Section("state", False, (f"{_count(len(records), 'job record')} in {path.name}",)))


def _made(dag: Dag | None) -> int:
    if dag is None:
        return 0
    return sum(1 for job in dag.jobs for output in job.output_paths if dag.path(output).exists())


def _unrecorded(name: str, made: int) -> Section:
    if not made:
        return Section("state", False, (f"no {name} yet: no job has been recorded",))
    return Section(
        "state",
        True,
        (
            f"no {name}, but {_count(made, 'output')} of the DAG exist; "
            "`spit-bash adopt` records them as current without running them",
        ),
    )


def _records(store: Store) -> dict[str, Record]:
    try:
        return dict(store.jobs)
    finally:
        store.close()


def _summary(run: runs.Run) -> dict | None:
    try:
        data = json.loads(run.summary.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def _last_run(layout: Layout) -> Section:
    for run in runs.history(layout.runs):
        summary = _summary(run)
        ended = None if summary is None else summary.get("ended")
        if ended in QUIET:
            continue
        if summary is None:
            return Section(
                "last run",
                True,
                (f"{run.name} ended without a summary: it is still running, or stopped before it could write one",),
            )
        return _ending(run, summary)
    return Section("last run", False, ("no run yet",))


def _ending(run: runs.Run, summary: dict) -> Section:
    ended = summary.get("ended")
    lines = [f"{run.name}: {ended}"]
    attention = ended != runs.Ending.FINISHED
    counted = [(key, summary.get(key)) for key in ("failed", "not_run", "blocked", "held")]
    for key, count in counted:
        if isinstance(count, int) and count:
            attention = True
            label = {"failed": "failed", "not_run": "did not run", "blocked": "blocked", "held": "held for review"}[key]
            lines.append(f"{_count(count, 'job')} {label}")
    if isinstance(summary.get("held"), int) and summary["held"]:
        lines.append("`spit-bash plan` names the review to make and where to record it")
    if any(isinstance(summary.get(key), int) and summary[key] for key in ("failed", "not_run")):
        lines.append("`spit-bash log` and `spit-bash explain ID` show why")
    if ended != runs.Ending.FINISHED:
        lines.append(f"its log is {run.log}")
    return Section("last run", attention, tuple(lines))


def _kept(layout: Layout) -> Section:
    lines = []
    for run in runs.history(layout.runs):
        found = runs.kept(run)
        if not found:
            continue
        taken = runs.size(path for each in found for _, _, path in each.files)
        lines.append(f"{run.name}: {amount(taken)} replaced by {_count(len(found), 'job')}")
    if lines:
        folder = shlex.quote(str(layout.folder))
        lines.append(
            f"`spit-bash restore --dir {folder} --run NAME` puts them back; "
            f"`spit-bash prune --dir {folder} --keep-runs N` deletes older runs"
        )
    else:
        lines.append("no replaced outputs kept")
    return Section("kept copies", False, tuple(lines))


def _leftovers(dag: Dag | None, failure: str | None, records: dict[str, Record] | None) -> Section:
    if failure is not None:
        return Section("leftovers", True, (f"not checked: the DAG could not be made: {failure}",))
    if dag is None:
        return Section("leftovers", False, ("not checked: name the DAG or recipe to check them",))
    if records is None:
        return Section("leftovers", False, ("not checked: the state cannot be read",))
    found = leftovers(dag, records)
    return Section("leftovers", False, tuple(leftover_lines(found, False, False)) or ("none",))
