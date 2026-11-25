from __future__ import annotations

from dataclasses import dataclass
from enum import Enum


class Status(str, Enum):
    PASS = "pass"
    WARN = "warn"
    FAIL = "fail"


class Name(str, Enum):
    COMMANDS = "slurm commands"
    PROFILE = "profile"
    LIMITS = "limits"
    QUEUE = "queue"
    PROBE = "probe"
    TOOLS = "tools"
    EXCLUSIVE = "exclusive create"
    VISIBILITY = "visibility"
    CLOCKS = "clocks"
    ACCOUNTING = "accounting"
    STOP_SIGNAL = "stop signal"
    ENFORCEMENT = "enforcement"
    MEMORY = "memory enforcement"
    TIME_LIMIT = "time limit"


@dataclass(frozen=True)
class Check:
    name: Name
    status: Status
    detail: str
