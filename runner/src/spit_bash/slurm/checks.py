"""What one check of the site check says."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum


class Status(str, Enum):
    """How one check came out."""

    # What a run relies on holds.
    PASS = "pass"
    # A run can go on, with less than it would otherwise have, or with a risk the report names.
    WARN = "warn"
    # A run would not be safe.
    FAIL = "fail"


class Name(str, Enum):
    """The assumptions the site check tests, as the report and the record name them."""

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
    """One assumption, and what the check found."""

    name: Name
    status: Status
    detail: str
