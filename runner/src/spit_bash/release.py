from __future__ import annotations

from . import runs
from .model import DagError, Layout
from .slurm import commands, intent
from .slurm.controller import Controller, folder
from .slurm.profile import Profile
from .slurm.survey import Reason, Survey, Unverified, survey
from .state.store import Store


def release(layout: Layout, yes: bool) -> int:
    slurm = folder(layout)
    if not any(slurm.glob("*.dispatch")):
        print(f"no Slurm reservations in {slurm}")
        return 0
    commands.require_local_cluster()
    state = Store(layout.state, True)
    controller = Controller(Profile(), layout)
    try:
        state.lock()
        controller.lease()
        surveyed = survey(slurm, controller.name, write=True)
        if surveyed is None:
            raise DagError("cannot read the Slurm queue with squeue; run again once squeue answers")
        return _settle(layout, surveyed, yes)
    finally:
        controller.close()
        state.close()


def _settle(layout: Layout, surveyed: Survey, yes: bool) -> int:
    if not surveyed.unverified:
        print("the runner verified every Slurm reservation of this folder; nothing to release")
        return 0
    if surveyed.queued:
        ids = sorted(task.id for task in surveyed.queued)
        print(
            f"squeue lists {len(ids)} task{'' if len(ids) == 1 else 's'} of this folder: {', '.join(ids)}. "
            "Nothing is released while any task of the folder is in the queue, since a reservation the runner "
            "cannot verify may belong to one of them. Wait for them to end, or cancel them:"
        )
        print(f"  scancel {' '.join(ids)}")
        return 1
    blocked = {each.intent for each in surveyed.unverified if each.reason is Reason.UNANSWERED}
    offered = [each for each in surveyed.unverified if each.intent not in blocked]
    lines = _listing(offered, [each for each in surveyed.unverified if each.intent in blocked])
    if not offered:
        for line in lines:
            print(line)
        return 1
    if not yes:
        for line in lines:
            print(line)
        print("run again with --yes to release them")
        return 0
    run = runs.start(layout.runs, runs.stamp())
    with runs.Recording(run):
        for line in lines:
            print(line)
        intents = sorted({each.intent for each in offered})
        for name in intents:
            surveyed.dispatches.clear(name)
            intent.remove(surveyed.dispatches.slurm, name)
        print(f"released {len(intents)} reservation{'' if len(intents) == 1 else 's'}, recorded in {run.folder}")
    runs.finish(run, runs.Ending.RELEASED, {"reservations": len(intents), "tasks": len(offered)})
    return 0


def _listing(offered: list[Unverified], kept: list[Unverified]) -> list[str]:
    lines = []
    if offered:
        lines.append(f"the runner cannot verify {len(offered)} Slurm reservation{'' if len(offered) == 1 else 's'}:")
        lines += [f"  {each.label}: {each.reason.value}" for each in offered]
        lines += [
            "squeue lists no task of this folder. That does not prove these tasks will never run: a task can be "
            "running where squeue does not show it.",
            "Releasing them says, on your word, that none of them is running unseen. Each one's batch is deleted "
            "with its reservation, so a task that starts later finds nothing to run and writes nothing. "
            "The runner keeps attempts.json and the submission history.",
            "The runner would verify them itself if sacct listed each task, with the comment it was submitted with.",
        ]
    if kept:
        lines.append("not offered, since only a fresh read can settle them; run again once sacct answers:")
        lines += [f"  {each.label}: {each.reason.value}" for each in kept]
    return lines

