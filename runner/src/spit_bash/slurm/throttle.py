from __future__ import annotations

import asyncio
import socket
import time
from collections import deque
from pathlib import Path
from typing import Annotated, NamedTuple

from pydantic import AfterValidator, ConfigDict, Field, FiniteFloat, TypeAdapter

from ..model import DagError
from . import intent
from .profile import Profile


def _ordered(sent: tuple[float, ...]) -> tuple[float, ...]:
    if tuple(sorted(sent)) != sent:
        raise ValueError("submission timestamps must be ordered")
    return sent


class RateHistory(NamedTuple):
    host: str
    origin: FiniteFloat
    period: Annotated[FiniteFloat, Field(gt=0)]
    sent: Annotated[tuple[FiniteFloat, ...], AfterValidator(_ordered)]
    pending: bool


HISTORY = TypeAdapter(RateHistory, config=ConfigDict(strict=True))


class SubmitRate:
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
            history = HISTORY.validate_json(self.path.read_bytes())
        except FileNotFoundError:
            return
        except (OSError, ValueError) as exc:
            raise DagError(f"cannot verify Slurm submission history {self.path}: {exc}") from exc
        if (
            history.pending
            or history.host != self.host
            or abs(history.origin - self.clock_origin) > 1
            or (history.sent and history.sent[-1] > now)
            or self.period > history.period
        ):
            self.resume_at = now + self.period
        else:
            self.sent.extend(history.sent)

    async def wait(self) -> None:
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
        self._save(pending=True)

    def settled(self) -> None:
        self.sent.append(time.monotonic())
        self._save(pending=False)

    def _save(self, *, pending: bool) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        history = RateHistory(self.host, self.clock_origin, self.period, tuple(self.sent), pending)
        intent.atomic(self.path, HISTORY.dump_json(history))
