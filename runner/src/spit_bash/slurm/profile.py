from __future__ import annotations

import math
import shlex
import sys
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any

from ..model import DagError, Job

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

TIME_UNITS = {"s": 1, "m": 60, "h": 3600, "d": 86400}
MEMORY_UNITS = {"K": 1024, "M": 1024**2, "G": 1024**3, "T": 1024**4}


def seconds(text: Any, name: str) -> float:
    if isinstance(text, (int, float)) and not isinstance(text, bool) and text >= 0 and math.isfinite(text):
        return float(text)
    if isinstance(text, str) and text[-1:].lower() in TIME_UNITS and text[:-1].isascii() and text[:-1].isdigit():
        try:
            return float(int(text[:-1]) * TIME_UNITS[text[-1:].lower()])
        except (OverflowError, ValueError):
            pass
    raise DagError(f"profile: {name} must be a time such as 90s, 30m, 4h or 2d, not {text!r}")


def memory(text: Any, name: str) -> int:
    if isinstance(text, int) and not isinstance(text, bool) and text > 0:
        return text
    if isinstance(text, str) and text[-1:].upper() in MEMORY_UNITS and text[:-1].isascii() and text[:-1].isdigit():
        return int(text[:-1]) * MEMORY_UNITS[text[-1:].upper()]
    raise DagError(f"profile: {name} must be a size such as 512M or 4G, not {text!r}")


@dataclass(frozen=True)
class Resources:
    account: str | None = None
    partition: str | None = None
    time: float = 3600.0
    memory: int = 1024**3
    options: tuple[str, ...] = ()
    setup: tuple[str, ...] = ()


@dataclass(frozen=True)
class Profile:
    resources: Resources = field(default_factory=Resources)
    operations: dict[str, dict[str, Any]] = field(default_factory=dict)
    stages: dict[str, dict[str, Any]] = field(default_factory=dict)
    time_factor: float = 3.0
    min_time: float = 300.0
    max_time: float | None = None
    jobs: int = 100
    submissions: int = 50
    per: float = 60.0
    array_limit: int | None = None
    status_interval: float = 120.0
    max_status_interval: float = 960.0
    result_interval: float = 2.0
    visibility: float = 60.0
    retries: int = 0
    memory_factor: float = 2.0
    retry_time_factor: float = 2.0
    max_memory: int | None = None
    local: frozenset[str] = frozenset()
    local_cores: int = 1
    short_job: float = 60.0
    command: tuple[str, ...] = (sys.executable, "-m", "spit_bash")
    require_site_check: bool = False

    def resources_for(self, job: Job) -> Resources:
        return self.resources_for_operation(job.operation, job.stage)

    def resources_for_operation(self, operation: str, stage: tuple[str, ...] = ()) -> Resources:
        chosen = self.resources
        for depth in range(1, len(stage) + 1):
            name = "/".join(stage[:depth])
            override = self.stages.get(name)
            if override is not None:
                chosen = _resources(override, f"stages.{name}", chosen)
        override = self.operations.get(operation)
        if override is not None:
            chosen = _resources(override, f"operations.{operation}", chosen)
        return chosen

    def time_for(self, job: Job, recorded: float | None, resources: Resources) -> float:
        if recorded is None:
            wanted = resources.time
        else:
            wanted = max(self.min_time, recorded * self.time_factor)
        return wanted if self.max_time is None else min(wanted, self.max_time)


def status_intervals(profile: Profile, min_job_age: float) -> tuple[float, float]:
    maximum = min(profile.max_status_interval, max(1.0, min_job_age / 2))
    return min(profile.status_interval, maximum), maximum


RESOURCE_KEYS = {"account", "partition", "time", "memory", "options", "setup"}
TOP_KEYS = RESOURCE_KEYS | {
    "operations", "stages", "time_factor", "min_time", "max_time", "jobs", "submit_rate", "array_limit",
    "status_interval", "max_status_interval", "result_interval", "visibility", "retries", "memory_factor",
    "retry_time_factor", "local", "local_cores", "command", "max_memory", "short_job", "require_site_check",
}

OWNED_OPTIONS = ("array", "clusters", "cluster-constraint", "federation", "job-name", "comment", "wrap", "parsable",
                 "requeue", "no-requeue", "test-only", "uid", "gid", "jobid", "output", "error")


def _strings(value: Any, name: str) -> tuple[str, ...]:
    if isinstance(value, str):
        return (value,)
    if isinstance(value, list) and all(isinstance(item, str) for item in value):
        return tuple(value)
    raise DagError(f"profile: {name} must be a string or a list of strings")


def _resources(table: dict[str, Any], where: str, base: Resources) -> Resources:
    unknown = set(table) - RESOURCE_KEYS
    if unknown:
        raise DagError(f"profile: {where} has unknown key {sorted(unknown)[0]!r}; it takes {', '.join(sorted(RESOURCE_KEYS))}")
    changes: dict[str, Any] = {}
    for key in ("account", "partition"):
        if key in table:
            if not isinstance(table[key], str):
                raise DagError(f"profile: {where}.{key} must be a string")
            changes[key] = table[key]
    if "time" in table:
        changes["time"] = seconds(table["time"], f"{where}.time")
    if "memory" in table:
        changes["memory"] = memory(table["memory"], f"{where}.memory")
    if "options" in table:
        options = _strings(table["options"], f"{where}.options")
        for option in options:
            key = option[2:].partition("=")[0]
            if not option.startswith("--") or not key or any(owned.startswith(key) for owned in OWNED_OPTIONS):
                raise DagError(f"profile: {where}.options cannot use {option!r}; use long sbatch resource "
                               "options such as --qos=short without overriding the runner's submission options")
        changes["options"] = options
    if "setup" in table:
        changes["setup"] = _strings(table["setup"], f"{where}.setup")
    return replace(base, **changes)


def _positive(value: Any, name: str, whole: bool = True) -> Any:
    kinds = (int,) if whole else (int, float)
    if isinstance(value, bool) or not isinstance(value, kinds) or value <= 0:
        raise DagError(f"profile: {name} must be a {'whole ' if whole else ''}number above 0")
    if not whole and not math.isfinite(value):
        raise DagError(f"profile: {name} must be finite")
    return value


def parse(table: dict[str, Any]) -> Profile:
    unknown = set(table) - TOP_KEYS
    if unknown:
        raise DagError(f"profile: unknown key {sorted(unknown)[0]!r}")
    resources = _resources({key: table[key] for key in RESOURCE_KEYS if key in table}, "profile", Resources())
    changes: dict[str, Any] = {"resources": resources}
    for key in ("operations", "stages"):
        named = table.get(key, {})
        if not isinstance(named, dict) or not all(isinstance(value, dict) for value in named.values()):
            raise DagError(f"profile: {key} must be tables of resources, as [{key}.NAME]")
        for name, override in named.items():
            _resources(override, f"{key}.{name}", resources)
        changes[key] = named
    for key in ("time_factor", "memory_factor", "retry_time_factor"):
        if key in table:
            changes[key] = float(_positive(table[key], key, whole=False))
    for key in ("min_time", "max_time", "status_interval", "max_status_interval", "result_interval", "visibility"):
        if key in table:
            changes[key] = seconds(table[key], key)
            if key in ("status_interval", "max_status_interval", "result_interval", "visibility") and changes[key] <= 0:
                raise DagError(f"profile: {key} must be above 0")
    for key in ("jobs", "array_limit", "local_cores"):
        if key in table:
            changes[key] = _positive(table[key], key)
    if "max_memory" in table:
        changes["max_memory"] = memory(table["max_memory"], "max_memory")
    if "short_job" in table:
        changes["short_job"] = seconds(table["short_job"], "short_job")
    if "retries" in table:
        if isinstance(table["retries"], bool) or not isinstance(table["retries"], int) or table["retries"] < 0:
            raise DagError("profile: retries must be a whole number, 0 or more")
        changes["retries"] = table["retries"]
    if "submit_rate" in table:
        rate = table["submit_rate"]
        count, _, per = rate.partition("/") if isinstance(rate, str) else ("", "", "")
        if not (count.isascii() and count.isdigit() and int(count) > 0 and per):
            raise DagError(f"profile: submit_rate must be as 50/1m, sbatch calls per time, not {rate!r}")
        changes["submissions"] = int(count)
        changes["per"] = seconds(per, "submit_rate")
        if changes["per"] <= 0:
            raise DagError("profile: submit_rate interval must be above 0")
    if "local" in table:
        changes["local"] = frozenset(_strings(table["local"], "local"))
    if "require_site_check" in table:
        if not isinstance(table["require_site_check"], bool):
            raise DagError("profile: require_site_check must be true or false")
        changes["require_site_check"] = table["require_site_check"]
    if "command" in table:
        command = table["command"]
        changes["command"] = tuple(shlex.split(command)) if isinstance(command, str) else _strings(command, "command")
    return Profile(**changes)


def load(path: Path | None) -> Profile:
    if path is None:
        return Profile()
    try:
        with path.open("rb") as stream:
            table = tomllib.load(stream)
    except OSError as exc:
        raise DagError(f"cannot read the profile {path}: {exc.strerror}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise DagError(f"cannot read the profile {path}: {exc}") from exc
    return parse(table)


def minutes(time: float) -> int:
    return max(1, math.ceil(time / 60))


def megabytes(size: int) -> int:
    return max(1, math.ceil(size / 1024**2))


def sbatch_options(resources: Resources, memory: int, time_limit: float) -> list[str]:
    return ["--cpus-per-task=1", f"--mem={megabytes(memory)}M", f"--time={minutes(time_limit)}", "--no-requeue",
            *([f"--account={resources.account}"] if resources.account else []),
            *([f"--partition={resources.partition}"] if resources.partition else []),
            *resources.options]
