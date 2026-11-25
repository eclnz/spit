from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Union

TOOL = "tool:"


class Part(Enum):
    COMMANDS = "commands"
    VERIFY = "verify"
    INPUTS = "inputs"
    OUTPUTS = "outputs"
    PATHS = "paths"


@dataclass(frozen=True, order=True)
class Tool:
    name: str


Changed = Union[Part, Tool]


def part_key(part: Changed) -> str:
    match part:
        case Part():
            return part.value
        case Tool(name):
            return f"{TOOL}{name}"


class Why(Enum):
    OUTPUT_MISSING = "output_missing"
    INPUT_MISSING = "input_missing"
    PENDING_SCREEN = "pending_screen"
    MISSING_MATCH = "missing_match"
    FORCED = "forced"
    NO_RECORD = "no_record"
    WORK_CHANGED = "work_changed"
    INPUT_CHANGED = "input_changed"
    OUTPUT_CHANGED = "output_changed"
    CHECKS_CHANGED = "checks_changed"
    CHECKS_NOT_RUN = "checks_not_run"
    CURRENT = "current"
    DEPENDENCY_WILL_RUN = "dependency_will_run"
    UPSTREAM_MAY_RUN = "upstream_may_run"
    NO_COMMAND_OUTPUT_MISSING = "no_command_output_missing"
    NO_COMMAND_DEPENDENCY_WILL_RUN = "no_command_dependency_will_run"
    NO_COMMAND_DEPENDENCY_MAY_RUN = "no_command_dependency_may_run"
    NO_COMMAND_OUTPUT_OLDER = "no_command_output_older"
    NO_COMMAND_CHECKING = "no_command_checking"
    NO_COMMAND_OUTPUTS_EXIST = "no_command_outputs_exist"
    MISSING_EXTERNAL = "missing_external"
    PROGRAM_NOT_FOUND = "program_not_found"
    HERE_NOT_FOUND = "here_not_found"
    DEPENDENCY_BLOCKED = "dependency_blocked"


@dataclass(frozen=True)
class Cause:
    why: Why
    paths: tuple[str, ...] = ()
    jobs: tuple[int, ...] = ()
    parts: tuple[Changed, ...] | None = ()
