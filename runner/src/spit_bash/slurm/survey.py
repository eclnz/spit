from __future__ import annotations

import asyncio
import shlex
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from . import commands
from .dispatch import Dispatches, terminal_accounting
from .executor import identify
from .intent import Manifests
from .reconcile import Task


class Reason(Enum):
    NO_END = "the queue no longer lists it, and accounting shows no end for it"
    UNANSWERED = "sacct did not answer"
    ACCEPTANCE = "the queue never showed whether Slurm accepted its submission"


@dataclass(frozen=True)
class Unverified:
    intent: str
    task: Task | None
    reason: Reason

    @property
    def label(self) -> str:
        return self.task.id if self.task is not None else f"submission {self.intent}"


@dataclass
class Survey:
    listed: tuple[Task, ...]
    tasks: tuple[Task, ...]
    unverified: tuple[Unverified, ...]
    dispatches: Dispatches
    manifests: Manifests

    @property
    def hidden(self) -> tuple[Task, ...]:
        return tuple(each.task for each in self.unverified if each.task is not None)

    @property
    def queued(self) -> tuple[Task, ...]:
        return tuple(task for task in self.listed if task.live)

    def problems(self) -> list[str]:
        by_reason: dict[Reason, list[str]] = {}
        for each in self.unverified:
            by_reason.setdefault(each.reason, []).append(each.label)
        return [
            f"cannot verify Slurm state for {', '.join(labels)}: {reason.value}" for reason, labels in by_reason.items()
        ]

    def advice(self, runner: Path) -> list[str]:
        reasons = {each.reason for each in self.unverified}
        found = []
        if Reason.UNANSWERED in reasons:
            found.append("run again once sacct answers")
        if reasons - {Reason.UNANSWERED}:
            found.append(f"`{release_command(runner)}` lists what the runner cannot verify and what releasing it means")
        return found


def release_command(runner: Path) -> str:
    return f"spit-bash release --dir {shlex.quote(str(runner))}"


def survey(slurm: Path, name: str, write: bool) -> Survey | None:
    listed = asyncio.run(commands.snapshot(name, (), "start"))
    if listed is None:
        return None
    manifests = Manifests(slurm)
    shown = identify(listed, manifests)
    dispatches = Dispatches(slurm)
    dispatches.reconcile(shown, write=write)
    accounted = asyncio.run(terminal_accounting(dispatches.missing(shown)))
    dispatches.reconcile(accounted.ended, write=write)
    tasks = shown + accounted.ended
    present = {(task.id, task.intent) for task in tasks}
    reason = Reason.NO_END if accounted.answered else Reason.UNANSWERED
    unverified = [Unverified(name, None, Reason.ACCEPTANCE) for name in dispatches.unknown]
    unverified += [
        Unverified(task.intent or "", task, reason)
        for task in sorted(dispatches.tasks().values(), key=lambda task: task.order)
        if (task.id, task.intent) not in present
    ]
    return Survey(shown, tasks, tuple(unverified), dispatches, manifests)
