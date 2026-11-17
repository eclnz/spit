"""The ledger: each task the controller submitted, and whether its end was dealt with.

Only a controller holding the state's lock writes it, so it has one writer, as the state does. A controller that
stops, however it stops, leaves the tasks it had not dealt with as pending, and the next one harvests their result
files or takes over the tasks still in the queue, rather than submitting them again.
"""

from __future__ import annotations

import json
import os
import time
from dataclasses import asdict, dataclass
from enum import Enum
from pathlib import Path
from typing import IO

from ..model import DagError


@dataclass(frozen=True)
class Entry:
    """One submitted task."""

    token: str
    # Slurm's id for the task, as `123_4`.
    task: str
    job: int
    key: str
    fingerprint: str
    attempt: int
    result: str
    # The run whose folder holds the job's log.
    run: str
    # Seconds since the epoch when it was submitted.
    at: float


class End(Enum):
    """How a task's end was dealt with."""

    # Its result was read, and recorded when the job succeeded.
    READ = "read"
    # It left the queue without a result, and the controller gave up waiting for one.
    LOST = "lost"
    # The controller cancelled it.
    CANCELLED = "cancelled"


class Ledger:
    """The ledger file of one runner folder: the tasks still pending, by token."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.pending: dict[str, Entry] = {}
        self._stream: IO[str] | None = None
        # Ends recorded since the last `flush`, written together rather than one sync each.
        self._ends: list[dict] = []
        try:
            text = path.read_text(encoding="utf-8")
        except FileNotFoundError:
            return
        except OSError as exc:
            raise DagError(f"cannot read the Slurm ledger {path}: {exc.strerror}") from exc
        # A controller killed mid-write leaves a partial last line, which records nothing whole and is ignored.
        for line in text.split("\n")[:-1]:
            try:
                item = json.loads(line)
                if "ended" in item:
                    self.pending.pop(item["ended"], None)
                else:
                    entry = Entry(**item)
                    self.pending[entry.token] = entry
            except (json.JSONDecodeError, TypeError) as exc:
                raise DagError(f"cannot read the Slurm ledger {path}: {exc}") from exc

    def submitted(self, entries: list[Entry]) -> None:
        """Record an array's tasks, all in one write and one sync, before the run waits on any of them."""
        for entry in entries:
            self.pending[entry.token] = entry
        self._append([asdict(entry) for entry in entries])

    def ended(self, token: str, how: End) -> None:
        """Record that a task's end was dealt with, at the next `flush`. Losing it to a crash costs nothing: the next
        run finds the task gone, with no result left to read, and asks sacct how it ended."""
        if self.pending.pop(token, None) is not None:
            self._ends.append({"ended": token, "how": how.value})

    def flush(self) -> None:
        if self._ends:
            ends, self._ends = self._ends, []
            self._append(ends)

    def _append(self, items: list[dict]) -> None:
        if not items:
            return
        if self._stream is None:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            self._stream = open(self.path, "a", encoding="utf-8")
        self._stream.write("".join(json.dumps(item) + "\n" for item in items))
        self._stream.flush()
        os.fsync(self._stream.fileno())

    def close(self) -> None:
        """Rewrite the ledger with only the pending tasks, so it does not grow run after run."""
        self.flush()
        if self._stream is not None:
            self._stream.close()
            self._stream = None
        if not self.path.exists():
            return
        temporary = self.path.with_name(f".{self.path.name}.{os.getpid()}")
        with open(temporary, "w", encoding="utf-8") as stream:
            for entry in self.pending.values():
                stream.write(json.dumps(asdict(entry)) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, self.path)


def now() -> float:
    return time.time()
