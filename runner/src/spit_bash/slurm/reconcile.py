from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Callable, Mapping

from .claim import name as claim_name


class Mode(Enum):
    RUN = "run"
    CHECK = "check"


@dataclass(frozen=True)
class Identity:
    key: str
    fingerprint: str
    mode: Mode


ENDED = frozenset({"BOOT_FAIL", "CANCELLED", "COMPLETED", "DEADLINE", "FAILED", "NODE_FAIL", "OUT_OF_MEMORY",
                   "TIMEOUT"})


@dataclass(frozen=True)
class Task:
    id: str
    intent: str | None
    index: int
    state: str

    @property
    def live(self) -> bool:
        return self.state not in ENDED

    @property
    def order(self) -> tuple[int, int]:
        array, _, index = self.id.partition("_")
        return int(array), int(index or 0)


@dataclass(frozen=True)
class Reconciled:
    waits: dict[str, Task]
    cancel: tuple[Task, ...]
    free: frozenset[str]


def reconcile(wanted: Mapping[str, Identity], snapshot: tuple[Task, ...],
              identify: Callable[[Task], Identity | None], claims: Mapping[str, str]) -> Reconciled:
    live: dict[str, list[tuple[Task, Identity]]] = {}
    cancel: list[Task] = []
    for task in snapshot:
        if not task.live:
            continue
        identity = identify(task)
        if identity is None:
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
    ended = {claim_name(task.intent, task.index): task.id for task in snapshot if not task.live and task.intent is not None}
    return frozenset(claim for claim, holder in claims.items() if ended.get(claim) == holder)


def newest(ids: list[str]) -> str:
    return max(ids, key=lambda id: Task(id, None, 0, "").order)
