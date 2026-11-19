"""A count of the Slurm commands a run sends, with the reason for each, kept in `call_logs.json`.

The file holds one record per run of the runner folder, newest last. It measures scheduler traffic only: a call is
recorded as it starts, whether or not Slurm answers it, and recording never changes what the run does.
"""

from __future__ import annotations

import json
import os
import time
from collections import Counter
from pathlib import Path

from . import intent

# Runs kept in the file, and calls kept in a run's timeline. Totals stay exact beyond the timeline's cap.
MAX_RUNS = 20
MAX_CALLS = 5000


class CallLog:
    """The calls of one run, saved to `path` as each is made."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.started = time.time()
        self.totals: Counter[str] = Counter()
        self.calls: list[list] = []
        self.dropped = 0
        self.runs = self._earlier()

    def _earlier(self) -> list[dict]:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return []
        runs = data.get("runs") if isinstance(data, dict) else None
        return [run for run in runs if isinstance(run, dict)][-(MAX_RUNS - 1):] if isinstance(runs, list) else []

    def record(self, tool: str, reason: str) -> None:
        """Count one call of `tool` made for `reason`, and save."""
        self.totals[f"{tool}:{reason}"] += 1
        self.totals[tool] += 1
        if len(self.calls) < MAX_CALLS:
            self.calls.append([round(time.time() - self.started, 3), tool, reason])
        else:
            self.dropped += 1
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            run = {"started": self.started, "pid": os.getpid(), "totals": dict(sorted(self.totals.items())),
                   "calls": self.calls, "dropped": self.dropped}
            intent.atomic(self.path, json.dumps({"runs": [*self.runs, run]}).encode())
        except OSError:
            # Measuring traffic must not stop the run that causes it.
            pass


# The log of the run in this process, set by `start`; None outside a run, as in tests.
active: CallLog | None = None


def start(path: Path) -> CallLog:
    """Begin the log of a run, replacing any earlier one in this process."""
    global active
    active = CallLog(path)
    return active


def record(tool: str, reason: str) -> None:
    """Count a call in the run's log, if a run has one."""
    if active is not None:
        active.record(tool, reason)
