from __future__ import annotations

from dataclasses import dataclass

from ..reuse.evidence import Compare
from ..runs import Run


@dataclass(frozen=True)
class Options:
    run: Run
    cores: int = 1
    memory: int | None = None
    compare: Compare = Compare.CONTENT
    keep_failed: bool = False
    enforce_memory: bool = True
    timestamp_logs: bool = False
    keep_replaced: bool = True

    @property
    def started(self) -> str:
        return self.run.name
