import json
import os
import time
from collections import Counter
from collections.abc import Iterator
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Literal, NamedTuple

FLUSH_INTERVAL = 60.0
MAX_CALLS = 5000

QueueReason = Literal["routine", "start", "cancel", "cleanup"]
Reason = Literal["routine", "start", "cancel", "cleanup", "submit", "classify", "probe", "config"]


class Command(Enum):
    SUBMIT = "sbatch"
    QUEUE = "squeue"
    ACCOUNTING = "sacct"
    CANCEL = "scancel"
    CONTROL = "scontrol"


class Call(NamedTuple):
    elapsed: float
    command: Command
    reason: Reason


@dataclass
class RunCalls:
    path: Path
    started: float = field(default_factory=time.time)
    clock: float = field(default_factory=time.monotonic)
    counts: Counter[tuple[Command, Reason]] = field(default_factory=Counter)
    pending: list[Call] = field(default_factory=list)
    written: int = 0
    last_write: float = float("-inf")


_current: ContextVar[RunCalls | None] = ContextVar("slurm_calls", default=None)


def record(command: Command, reason: Reason) -> None:
    run = _current.get()
    if run is None:
        return
    now = time.monotonic()
    run.counts[command, reason] += 1
    if run.written + len(run.pending) < MAX_CALLS:
        run.pending.append(Call(round(now - run.clock, 3), command, reason))
    if now - run.last_write >= FLUSH_INTERVAL:
        run.last_write = now
        append(run, final=False)


def append(run: RunCalls, *, final: bool) -> None:
    if not run.counts:
        return
    totals: Counter[str] = Counter()
    for (command, reason), count in run.counts.items():
        totals[command.value] += count
        totals[f"{command.value}:{reason}"] = count
    calls = [(call.elapsed, call.command.value, call.reason) for call in run.pending]
    recorded = run.written + len(calls)
    entry = {"started": run.started, "pid": os.getpid(), "final": final, "totals": dict(sorted(totals.items())),
             "calls": calls, "dropped": run.counts.total() - recorded}
    try:
        run.path.parent.mkdir(parents=True, exist_ok=True)
        with run.path.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(entry) + "\n")
    except OSError:
        return
    run.written = recorded
    run.pending.clear()


@contextmanager
def capture(path: Path) -> Iterator[None]:
    run = RunCalls(path)
    token = _current.set(run)
    try:
        yield
    finally:
        _current.reset(token)
        append(run, final=True)
