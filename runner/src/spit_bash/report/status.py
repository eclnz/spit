from __future__ import annotations

import json
from dataclasses import dataclass

from .. import runs
from ..leftovers import amount, leftovers, lines as leftover_lines
from ..model import Dag, DagError, Layout
from ..slurm import commands, site
from ..slurm.controller import folder as slurm_folder
from ..slurm.executor import job_name
from ..slurm.profile import Profile
from ..slurm.survey import survey
from ..state.store import Store

QUIET = (runs.Ending.RESTORED, runs.Ending.RELEASED)


@dataclass(frozen=True)
class Section:
    name: str
    attention: bool
    lines: tuple[str, ...]


def sections(layout: Layout, dag: Dag | None, failure: str | None, profile: Profile | None) -> list[Section]:
    records, state = _state(layout, dag)
    found = [_slurm(layout, profile), state, _last_run(layout), _kept(layout)]
    found.append(_leftovers(dag, failure, records))
    return found


def text(found: list[Section]) -> list[str]:
    lines = []
    for section in found:
        lines.append(f"{section.name}: {'needs attention' if section.attention else 'ok'}")
        lines += [f"  {line}" for line in section.lines]
    return lines


def _slurm(layout: Layout, profile: Profile | None) -> Section:
    slurm = slurm_folder(layout)
    lines: list[str] = []
    attention = False
    if not any(slurm.glob("*.dispatch")):
        lines.append("no Slurm reservations")
    else:
        try:
            commands.require_local_cluster()
            surveyed = survey(slurm, job_name(layout.folder), write=False)
        except DagError as exc:
            surveyed = None
            lines.append(str(exc))
        if surveyed is None:
            attention = True
            lines.append("cannot read the Slurm queue with squeue, so its reservations are not checked")
        else:
            queued = surveyed.queued
            if queued:
                lines.append(
                    f"{len(queued)} Slurm task{'' if len(queued) == 1 else 's'} of this folder "
                    f"{'is' if len(queued) == 1 else 'are'} queued or running"
                )
            if surveyed.unverified:
                attention = True
                lines += surveyed.problems() + surveyed.advice(layout.folder)
            elif not queued:
                lines.append("every reservation is settled")
    report = site.load(layout, profile)
    if profile is not None:
        verdict = site.verdict(report, profile)
        if verdict is not None:
            attention = attention or verdict.blocks
            lines.append(verdict.message)
        else:
            lines.append("the site check for this profile passed")
    elif report is not None:
        if report.failed:
            attention = True
            listing = "; ".join(f"{check.name.value}: {check.detail}" for check in report.failed)
            lines.append(f"the latest site check failed: {listing}; `spit-bash check-site` again once it is fixed")
        else:
            lines.append("the latest site check passed")
    return Section("slurm", attention, tuple(lines))


def _state(layout: Layout, dag: Dag | None) -> tuple[dict | None, Section]:
    path = layout.state
    if not path.exists():
        made = [] if dag is None else [output for job in dag.jobs for output in job.output_paths if dag.path(output).exists()]
        if made:
            return {}, Section(
                "state",
                True,
                (
                    f"no {path.name}, but {len(made)} output{'' if len(made) == 1 else 's'} of the DAG exist; "
                    "`spit-bash adopt` records them as current without running them",
                ),
            )
        return {}, Section("state", False, (f"no {path.name} yet: no job has been recorded",))
    try:
        store = Store(path, False)
    except DagError as exc:
        return None, Section("state", True, (str(exc),))
    try:
        jobs = dict(store.jobs)
    finally:
        store.close()
    return jobs, Section("state", False, (f"{len(jobs)} job record{'' if len(jobs) == 1 else 's'} in {path.name}",))


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
            lines.append(f"{count} job{'' if count == 1 else 's'} {label}")
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
        lines.append(f"{run.name}: {amount(taken)} replaced by {len(found)} job{'' if len(found) == 1 else 's'}")
    if lines:
        lines.append("`spit-bash restore --run NAME` puts them back; `spit-bash prune --keep-runs N` deletes older runs")
    else:
        lines.append("no replaced outputs kept")
    return Section("kept copies", False, tuple(lines))


def _leftovers(dag: Dag | None, failure: str | None, records: dict | None) -> Section:
    if failure is not None:
        return Section("leftovers", True, (f"not checked: the DAG could not be made: {failure}",))
    if dag is None:
        return Section("leftovers", False, ("not checked: name the DAG or recipe to check them",))
    if records is None:
        return Section("leftovers", False, ("not checked: the state cannot be read",))
    found = leftovers(dag, records)
    return Section("leftovers", False, tuple(leftover_lines(found, False, False)) or ("none",))
