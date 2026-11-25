from __future__ import annotations

import asyncio
from contextlib import suppress
import getpass
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

from ..model import DagError
from . import calllog

TIMEOUT = 60.0

BUSY = ("MaxSubmitJob", "QOSMaxSubmitJob", "AssocMaxSubmitJob", "Job violates accounting/QOS policy")
REFUSED = (
    "Invalid account",
    "Invalid partition",
    "invalid partition",
    "Invalid qos",
    "Invalid QOS",
    "Requested node configuration",
    "Invalid job array",
    "Invalid time limit",
    "Unrecognized option",
    "unrecognized option",
    "Unable to open file",
)


def require_local_cluster() -> None:
    for key in ("SLURM_CLUSTERS", "SBATCH_CLUSTERS", "SQUEUE_CLUSTERS", "SACCT_CLUSTERS"):
        if os.environ.get(key):
            raise DagError(f"Slurm runner uses one local cluster; unset {key} and connect to the intended cluster")


class SlurmError(Exception):
    pass


class Busy(SlurmError):
    pass


class Unsure(SlurmError):
    pass


@dataclass(frozen=True)
class Listed:
    task: str
    index: int
    state: str
    command: str


async def call(args: list[str], timeout: float = TIMEOUT) -> tuple[int, str, str]:
    try:
        process = await asyncio.create_subprocess_exec(
            *args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
    except OSError as exc:
        raise SlurmError(f"cannot run {args[0]}: {exc.strerror}; is Slurm on PATH?") from exc
    try:
        out, err = await asyncio.wait_for(process.communicate(), timeout)
    except asyncio.TimeoutError:
        with suppress(ProcessLookupError):
            process.kill()
        await process.wait()
        raise Unsure(f"{args[0]} did not answer within {timeout:.0f}s") from None
    except asyncio.CancelledError:
        with suppress(ProcessLookupError):
            process.kill()
        await process.wait()
        raise
    assert process.returncode is not None, "a process that was waited for has a status"
    return process.returncode, out.decode(errors="replace"), err.decode(errors="replace")


async def submit(script: Path, options: list[str]) -> str:
    calllog.record(calllog.Command.SUBMIT, "submit")
    code, out, err = await call(["sbatch", "--parsable", *options, str(script)])
    if code == 0 and out.strip():
        reply = out.strip().splitlines()[-1]
        if reply.isascii() and reply.isdigit():
            return reply
        raise Unsure(f"sbatch returned {reply!r}; cannot reconcile a non-local or invalid Slurm job ID")
    words = err.strip() or out.strip() or f"exited with status {code}"
    if code > 0 and any(mark in words for mark in BUSY):
        raise Busy(words)
    if code > 0 and any(mark in words for mark in REFUSED):
        raise SlurmError(f"sbatch refused the submission: {words}")
    raise Unsure(f"sbatch acceptance is unknown: {words}")


def _snapshot_args(name: str, tasks: tuple[str, ...] = ()) -> list[str]:
    selected = ["--jobs=" + ",".join(tasks)] if tasks else []
    return [
        "squeue",
        "--local",
        "-u",
        getpass.getuser(),
        f"--name={name}",
        *selected,
        "-r",
        "-t",
        "all",
        "-h",
        "-o",
        "%i|%K|%T|%o",
    ]


def _listing(out: str) -> list[Listed]:
    found = []
    for line in out.splitlines():
        task, _, rest = line.strip().partition("|")
        index, _, rest = rest.partition("|")
        state, _, command = rest.partition("|")
        array, separator, element = task.partition("_")
        if array.isascii() and array.isdigit() and (not separator or (element.isascii() and element.isdigit())):
            found.append(
                Listed(
                    task,
                    int(index) if index.isascii() and index.isdigit() else -1,
                    state.split()[0] if state else "",
                    command,
                )
            )
    return found


async def snapshot(
    name: str, tasks: tuple[str, ...] = (), reason: calllog.QueueReason = "routine"
) -> list[Listed] | None:
    calllog.record(calllog.Command.QUEUE, reason)
    try:
        code, out, _ = await call(_snapshot_args(name, tasks))
    except SlurmError:
        return None
    return _listing(out) if code == 0 else None


def snapshot_now(
    name: str, tasks: tuple[str, ...] = (), reason: calllog.QueueReason = "cleanup"
) -> list[Listed] | None:
    calllog.record(calllog.Command.QUEUE, reason)
    try:
        done = subprocess.run(_snapshot_args(name, tasks), capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return _listing(done.stdout) if done.returncode == 0 else None


async def accounting(tasks: list[str], *, intents: dict[str, str] | None = None) -> dict[str, str] | None:
    if not tasks:
        return {}
    calllog.record(calllog.Command.ACCOUNTING, "classify")
    fields = ["--format=JobID,State"] if intents is None else ["--duplicates", "--format=JobID,State,Comment%64"]
    try:
        code, out, _ = await call(["sacct", "--local", "-X", "-n", "-P", "-j", ",".join(tasks), *fields])
    except SlurmError:
        return None
    if code != 0:
        return None
    found = {}
    for line in out.splitlines():
        task, _, rest = line.strip().partition("|")
        state, _, comment = rest.partition("|")
        if intents is not None and (task not in intents or comment.strip() != f"spit:{intents[task]}"):
            continue
        if task:
            found[task] = state.split()[0] if state.split() else state
    return found


def cancel(tasks: list[str]) -> list[str]:
    return list(cancel_report(tasks))


def cancel_report(tasks: list[str]) -> dict[str, str]:
    failed = {}
    for start in range(0, len(tasks), 500):
        chunk = tasks[start : start + 500]
        message = _scancel(chunk)
        if message is None:
            continue
        if message == UNANSWERED or len(chunk) == 1:
            if not (len(chunk) == 1 and _gone(message)):
                failed.update(dict.fromkeys(chunk, message))
            continue
        for task in chunk:
            message = _scancel([task])
            if message is not None and not _gone(message):
                failed[task] = message
    return failed


UNANSWERED = "scancel did not answer in time"


def _scancel(tasks: list[str]) -> str | None:
    calllog.record(calllog.Command.CANCEL, "cancel")
    try:
        done = subprocess.run(
            ["scancel", *tasks], capture_output=True, text=True, timeout=10, env=_cancel_environment()
        )
    except subprocess.TimeoutExpired:
        return UNANSWERED
    except OSError as exc:
        return f"scancel could not run: {exc}"
    if done.returncode == 0:
        return None
    lines = [line.strip() for line in done.stderr.splitlines() if line.strip()]
    return lines[-1].removeprefix("scancel: error: ") if lines else f"scancel exited {done.returncode}"


def _gone(message: str) -> bool:
    return "invalid job id" in message.lower()


@dataclass(frozen=True)
class Config:
    max_array: int = 1001
    min_job_age: float = 300.0


def cancel_name(name: str) -> bool:
    calllog.record(calllog.Command.CANCEL, "probe")
    try:
        done = subprocess.run(
            ["scancel", "-u", getpass.getuser(), f"--name={name}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=10,
            env=_cancel_environment(),
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.returncode == 0


def _cancel_environment() -> dict[str, str]:
    return {key: value for key, value in os.environ.items() if not key.startswith("SCANCEL_")}


def site_values() -> dict[str, str] | None:
    calllog.record(calllog.Command.CONTROL, "config")
    try:
        done = subprocess.run(["scontrol", "show", "config"], capture_output=True, text=True, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return _site_values(done.stdout)


async def check_site_values() -> dict[str, str] | None:
    try:
        code, stdout, _ = await call(["scontrol", "show", "config"])
    except SlurmError:
        return None
    return _site_values(stdout) if code == 0 else None


def _site_values(stdout: str) -> dict[str, str]:
    values = {}
    for line in stdout.splitlines():
        name, _, value = line.partition("=")
        values[name.strip()] = value.strip().split()[0] if value.strip() else ""
    return values


def config() -> Config:
    values = site_values() or {}
    found = Config()
    array = values.get("MaxArraySize", "")
    age = values.get("MinJobAge", "")
    return Config(
        int(array) if array.isdigit() else found.max_array, float(age) if age.isdigit() else found.min_job_age
    )
