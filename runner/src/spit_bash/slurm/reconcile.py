"""What a run does about the tasks in the queue, as one pure function of what it wants and what Slurm shows.

A task's identity is its job's: the key of what it makes, the fingerprint of its work, and whether it runs the job
or only its checks. The queue shows each task's intent and index, from which the intent's manifest gives that
identity, so a run knows every task of its runner folder, whichever controller submitted it and however the pipeline
has changed since. `plan` calls `reconcile` too, and acts on nothing.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Callable, Mapping


class Mode(Enum):
    """What a task does with its job."""

    # Its checks, `verify` commands and commands, as a run does.
    RUN = "run"
    # Its checks alone, on the files it has.
    CHECK = "check"


@dataclass(frozen=True)
class Identity:
    """What a task does: the job it makes, the work, and the mode. Two tasks with one identity do the same thing."""

    key: str
    fingerprint: str
    mode: Mode


# The states in which a task has left Slurm's control for good. Any other state, such as PENDING, RUNNING or
# COMPLETING, is a task that may still run commands.
ENDED = frozenset({"BOOT_FAIL", "CANCELLED", "COMPLETED", "DEADLINE", "FAILED", "NODE_FAIL", "OUT_OF_MEMORY",
                   "PREEMPTED", "TIMEOUT"})


@dataclass(frozen=True)
class Task:
    """A task as squeue lists it."""

    # As `123_4`.
    id: str
    # The content hash naming its intent, or None for a task whose script is no intent of this folder's.
    intent: str | None
    index: int
    state: str

    @property
    def live(self) -> bool:
        return self.state not in ENDED

    @property
    def order(self) -> tuple[int, int]:
        """Its place in Slurm's own order of submission: array id, then index."""
        array, _, index = self.id.partition("_")
        return int(array), int(index or 0)


@dataclass(frozen=True)
class Reconciled:
    """What to do about the queue."""

    # The task each wanted key waits on.
    waits: dict[str, Task]
    # Live tasks to cancel, in Slurm's order.
    cancel: tuple[Task, ...]
    # Wanted keys with no live task, free to submit.
    free: frozenset[str]


def reconcile(wanted: Mapping[str, Identity], snapshot: tuple[Task, ...],
              identify: Callable[[Task], Identity | None], claims: Mapping[str, str]) -> Reconciled:
    """Decide, from what the run wants, the queue, and the claims, which task each key waits on and which to cancel.

    `wanted` is the identity the plan runs for each key; `claims` the task holding each key's claim. For each key:

    - a live task doing the wanted work is waited on: the one holding the key's claim, else the first Slurm took;
    - every other live task of the key is cancelled: older work, a twin, or work the plan does not run;
    - a live task whose intent this folder does not hold is cancelled, since nothing could say what it does;
    - a wanted key with no live task is free.

    A key is never submitted while any of its tasks is live, so two live tasks of one key are a fault, never a retry,
    and keeping one of them is safe.
    """
    live: dict[str, list[tuple[Task, Identity]]] = {}
    cancel: list[Task] = []
    for task in snapshot:
        if not task.live:
            continue
        identity = identify(task)
        if identity is None:
            cancel.append(task)
            continue
        live.setdefault(identity.key, []).append((task, identity))
    waits: dict[str, Task] = {}
    for key, tasks in live.items():
        want = wanted.get(key)
        doing = sorted((task for task, identity in tasks if identity == want), key=lambda task: task.order)
        if doing:
            holder = next((task for task in doing if claims.get(key) == task.id), doing[0])
            waits[key] = holder
        cancel.extend(task for task, _ in tasks if task is not waits.get(key))
    return Reconciled(
        waits=waits,
        cancel=tuple(sorted(cancel, key=lambda task: task.order)),
        free=frozenset(key for key in wanted if key not in live),
    )


def releasable(claims: Mapping[str, str], snapshot: tuple[Task, ...]) -> frozenset[str]:
    """The claims no live task holds, which can go: their holder ended, or was never this folder's.

    `claims` must be listed before `snapshot` is read, so that a task that took a claim after the listing, being live
    when the snapshot was read, is never taken for one that ended.
    """
    live = {task.id for task in snapshot if task.live}
    return frozenset(claim for claim, holder in claims.items() if holder not in live)


def newest(ids: list[str]) -> str:
    """The task Slurm took last of `ids`, whose result stands when several tasks of one key left one."""
    return max(ids, key=lambda id: Task(id, None, 0, "").order)
