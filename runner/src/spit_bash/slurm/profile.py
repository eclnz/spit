"""A Slurm profile: what is specific to a cluster or a project, read from a TOML file, so a pipeline stays the same
on a laptop and on a cluster."""

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
else:  # pragma: no cover - the oldest Python the runner supports
    import tomli as tomllib

TIME_UNITS = {"s": 1, "m": 60, "h": 3600, "d": 86400}
MEMORY_UNITS = {"K": 1024, "M": 1024**2, "G": 1024**3, "T": 1024**4}


def seconds(text: Any, name: str) -> float:
    """A length of time as a profile gives it: seconds as a number, or a whole number and s, m, h or d."""
    if isinstance(text, (int, float)) and not isinstance(text, bool) and text >= 0 and math.isfinite(text):
        return float(text)
    if isinstance(text, str) and text[-1:].lower() in TIME_UNITS and text[:-1].isascii() and text[:-1].isdigit():
        try:
            return float(int(text[:-1]) * TIME_UNITS[text[-1:].lower()])
        except (OverflowError, ValueError):
            pass
    raise DagError(f"profile: {name} must be a time such as 90s, 30m, 4h or 2d, not {text!r}")


def memory(text: Any, name: str) -> int:
    """A size as a profile gives it: bytes as a number, or a whole number and K, M, G or T."""
    if isinstance(text, int) and not isinstance(text, bool) and text > 0:
        return text
    if isinstance(text, str) and text[-1:].upper() in MEMORY_UNITS and text[:-1].isascii() and text[:-1].isdigit():
        return int(text[:-1]) * MEMORY_UNITS[text[-1:].upper()]
    raise DagError(f"profile: {name} must be a size such as 512M or 4G, not {text!r}")


@dataclass(frozen=True)
class Resources:
    """What one operation's jobs ask Slurm for, beyond the cores and memory the DAG gives each job."""

    account: str | None = None
    partition: str | None = None
    # `--time` for a job with no recorded time, in seconds.
    time: float = 600.0
    # Memory for a job whose operation declares none, in bytes.
    memory: int = 1024**3
    # More `sbatch` options, each one argument, as `--qos=short`.
    options: tuple[str, ...] = ()
    # Shell lines a job runs before spit-bash, such as `module load fsl/6.0`.
    setup: tuple[str, ...] = ()


@dataclass(frozen=True)
class Profile:
    """How a run uses Slurm. `resources` are the defaults; `operations` and `stages` override them by name."""

    resources: Resources = field(default_factory=Resources)
    operations: dict[str, dict[str, Any]] = field(default_factory=dict)
    stages: dict[str, dict[str, Any]] = field(default_factory=dict)
    # A job's recorded time times this is its `--time`, within `min_time` and `max_time`.
    time_factor: float = 3.0
    min_time: float = 300.0
    max_time: float | None = None
    # At most this many tasks queued or running at once.
    jobs: int = 100
    # At most `submissions` calls to sbatch in any `per` seconds.
    submissions: int = 50
    per: float = 60.0
    # The most tasks of one array that run at once, as `--array=...%N`; None for no limit.
    array_limit: int | None = None
    # How often the queue is read, in one squeue call for the whole run, while its tasks are changing.
    status_interval: float = 120.0
    # The longest gap between reads, which doubles from `status_interval` while reads find nothing new. Both
    # targets are capped at half the site's MinJobAge.
    max_status_interval: float = 960.0
    # How often the results folder is read, and ready jobs submitted.
    result_interval: float = 2.0
    # The shared filesystem's visibility bound: how long a file one node wrote may stay unseen, or seen as it was, on
    # another. One bound for results, inputs and outputs alike.
    visibility: float = 60.0
    # How many times a job Slurm killed for memory or time is submitted again, and how much more it gets each time.
    retries: int = 2
    memory_factor: float = 2.0
    retry_time_factor: float = 2.0
    # The most memory a retry asks for, in bytes; None for no limit beyond the cluster's.
    max_memory: int | None = None
    # Operations whose jobs the controller runs itself, as a local run does, on `local_cores` cores.
    local: frozenset[str] = frozenset()
    local_cores: int = 1
    # Jobs that take less than this on average, as Slurm jobs, are named after a run, as better run locally.
    short_job: float = 60.0
    # How a node starts spit-bash, as arguments; by default, the Python running the controller.
    command: tuple[str, ...] = (sys.executable, "-m", "spit_bash")
    # Whether a run needs a site check that was made for this profile, as `spit-bash check-site` makes one. A check
    # that failed stops a run whether or not this is set.
    require_site_check: bool = False

    def resources_for(self, job: Job) -> Resources:
        """The defaults, then the job's stage's overrides from outermost to innermost, then its operation's."""
        return self.resources_for_operation(job.operation, job.stage)

    def resources_for_operation(self, operation: str, stage: tuple[str, ...] = ()) -> Resources:
        """Apply the stages from outermost to innermost, then the operation."""
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
        """The `--time` a job asks for, in seconds: its recorded time times `time_factor`, within the bounds,
        or its resources' time when it has no record."""
        if recorded is None:
            wanted = resources.time
        else:
            wanted = max(self.min_time, recorded * self.time_factor)
        return wanted if self.max_time is None else min(wanted, self.max_time)


def status_intervals(profile: Profile, min_job_age: float) -> tuple[float, float]:
    """Return the base and maximum target gaps for this site's queue."""
    maximum = min(profile.max_status_interval, max(1.0, min_job_age / 2))
    return min(profile.status_interval, maximum), maximum


RESOURCE_KEYS = {"account", "partition", "time", "memory", "options", "setup"}
TOP_KEYS = RESOURCE_KEYS | {
    "operations", "stages", "time_factor", "min_time", "max_time", "jobs", "submit_rate", "array_limit",
    "status_interval", "max_status_interval", "result_interval", "visibility", "retries", "memory_factor",
    "retry_time_factor", "local", "local_cores", "command", "max_memory", "short_job", "require_site_check",
}

# These options control admission, identity, routing or terminal-state semantics.
# Reject abbreviations as well, since Slurm accepts unambiguous long prefixes.
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
    """A profile from its TOML table. Raises `DagError` naming the key that is wrong."""
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
    """The profile at `path`, or the defaults when there is none."""
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
    """A time in seconds as Slurm's `--time` takes it, in whole minutes, rounded up, at least 1."""
    return max(1, math.ceil(time / 60))


def megabytes(size: int) -> int:
    """A size in bytes as Slurm's `--mem` takes it, in whole MiB, rounded up."""
    return max(1, math.ceil(size / 1024**2))


def sbatch_options(resources: Resources, memory: int, time_limit: float) -> list[str]:
    """The `sbatch` options of one request for a single core, `memory` bytes and `time_limit` seconds, as a run
    asks for a task."""
    return ["--cpus-per-task=1", f"--mem={megabytes(memory)}M", f"--time={minutes(time_limit)}", "--no-requeue",
            *([f"--account={resources.account}"] if resources.account else []),
            *([f"--partition={resources.partition}"] if resources.partition else []),
            *resources.options]
