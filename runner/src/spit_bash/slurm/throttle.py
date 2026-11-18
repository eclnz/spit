"""The rolling sbatch allowance for one runner folder and cluster."""

from __future__ import annotations

import asyncio
import json
import math
import socket
import time
from collections import deque
from pathlib import Path

from ..model import DagError
from . import intent
from .profile import Profile


class SubmitRate:
    """Charge calls before dispatch, including refusals and unanswered requests."""

    def __init__(self, folder: Path, profile: Profile) -> None:
        self.path = folder / "submit-rate.json"
        self.limit = profile.submissions
        self.period = profile.per
        self.host = socket.gethostname()
        now = time.monotonic()
        self.clock_origin = time.time() - now
        self.sent: deque[float] = deque()
        self.resume_at = now
        try:
            host, origin, previous_period, sent, pending = json.loads(self.path.read_text(encoding="utf-8"))
            if (not isinstance(host, str) or not isinstance(sent, list)
                    or not all(isinstance(value, (int, float)) and math.isfinite(value)
                               for value in (origin, previous_period, *sent))
                    or previous_period <= 0 or sent != sorted(sent) or type(pending) is not bool):
                raise ValueError("invalid submission history")
        except FileNotFoundError:
            return
        except (OSError, TypeError, ValueError) as exc:
            raise DagError(f"cannot verify Slurm submission history {self.path}: {exc}") from exc
        # Process monotonic timestamps share a clock only on the same boot. A longer new window
        # can include timestamps the old one was already allowed to discard.
        if (pending or host != self.host or abs(origin - self.clock_origin) > 1
                or (sent and sent[-1] > now) or self.period > previous_period):
            self.resume_at = now + self.period
        else:
            self.sent.extend(sent)

    async def wait(self) -> None:
        """Wait for both the rolling limit and the spacing between calls."""
        gap = self.period / self.limit
        while True:
            now = time.monotonic()
            while self.sent and now - self.sent[0] >= self.period:
                self.sent.popleft()
            at = self.resume_at
            if self.sent:
                at = max(at, self.sent[-1] + gap)
            if len(self.sent) >= self.limit:
                at = max(at, self.sent[-self.limit] + self.period)
            if at <= now:
                return
            await asyncio.sleep(at - now)

    def charge(self) -> None:
        """Save the call debit before the process can send sbatch."""
        self._save(pending=True)

    def settled(self) -> None:
        """Space the next call from this call's return, including any dispatch latency."""
        self.sent.append(time.monotonic())
        self._save(pending=False)

    def _save(self, *, pending: bool) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        intent.atomic(self.path, json.dumps([self.host, self.clock_origin, self.period, list(self.sent), pending]).encode())
