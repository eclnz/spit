"""The Slurm commands the controller runs, and what it reads from their output.

Each call has a timeout, as a busy `slurmctld` may not answer. The run reads the queue in one `squeue` call for all
its tasks, filtered by the runner folder's job name, and asks `sacct` only about tasks that left the queue without a
result. `sacct` is optional: with accounting off or its database down, the run goes on without the reasons it gives.
"""

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

# How long one call may take before the controller gives up on it.
TIMEOUT = 60.0

# What sbatch says when the user's queue or the site's limits are full: wait and submit again.
BUSY = ("MaxSubmitJob", "QOSMaxSubmitJob", "AssocMaxSubmitJob", "Job violates accounting/QOS policy")
# Definitive rejections leave no array to reconcile.
REFUSED = ("Invalid account", "Invalid partition", "invalid partition", "Invalid qos", "Invalid QOS",
           "Requested node configuration", "Invalid job array", "Invalid time limit", "Unrecognized option",
           "unrecognized option", "Unable to open file")


def require_local_cluster() -> None:
    """Reject environment routing that can make submission and reconciliation use different clusters."""
    for key in ("SLURM_CLUSTERS", "SBATCH_CLUSTERS", "SQUEUE_CLUSTERS", "SACCT_CLUSTERS"):
        if os.environ.get(key):
            raise DagError(f"Slurm runner uses one local cluster; unset {key} and connect to the intended cluster")


class SlurmError(Exception):
    """A Slurm command could not be run, or failed in a way the run cannot get past."""


class Busy(SlurmError):
    """sbatch refused a submission because a queue limit is full; it may be submitted again later."""


class Unsure(SlurmError):
    """sbatch failed without saying whether it accepted the submission."""


@dataclass(frozen=True)
class Listed:
    """A task as `squeue -t all` lists it."""

    # As `123_4`.
    task: str
    index: int
    state: str
    # The command Slurm runs: the task's script.
    command: str


async def call(args: list[str], timeout: float = TIMEOUT) -> tuple[int, str, str]:
    """Run a command; its status, standard output and standard error. Raises `Unsure` when it times out."""
    try:
        process = await asyncio.create_subprocess_exec(*args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                                       stderr=subprocess.PIPE)
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
        # A run being stopped: the call is not left running, unseen, to submit or cancel after the run has gone.
        with suppress(ProcessLookupError):
            process.kill()
        await process.wait()
        raise
    assert process.returncode is not None, "a process that was waited for has a status"
    return process.returncode, out.decode(errors="replace"), err.decode(errors="replace")


async def submit(script: Path, options: list[str]) -> str:
    """Submit an array; its Slurm job id. Raises `Busy`, `Unsure` or `SlurmError`."""
    calllog.record("sbatch", "submit")
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
    return ["squeue", "--local", "-u", getpass.getuser(), f"--name={name}", *selected, "-r", "-t", "all", "-h",
            "-o", "%i|%K|%T|%o"]


def _listing(out: str) -> list[Listed]:
    found = []
    for line in out.splitlines():
        task, _, rest = line.strip().partition("|")
        index, _, rest = rest.partition("|")
        state, _, command = rest.partition("|")
        array, separator, element = task.partition("_")
        if array.isascii() and array.isdigit() and (not separator or (element.isascii() and element.isdigit())):
            found.append(Listed(task, int(index) if index.isascii() and index.isdigit() else -1,
                                state.split()[0] if state else "", command))
    return found


async def snapshot(name: str, tasks: tuple[str, ...] = (), reason: str = "routine") -> list[Listed] | None:
    """Every task of this user's named `name`, queued, running or ended within MinJobAge, in one call; None when
    squeue fails.

    slurmctld filters by the name, so the call lists the runner folder's tasks only, whatever pipeline submitted them.
    `reason` tags the call in the call log: `routine`, `expedited`, `start` or `cancel`.
    """
    calllog.record("squeue", reason)
    try:
        code, out, _ = await call(_snapshot_args(name, tasks))
    except SlurmError:
        return None
    return _listing(out) if code == 0 else None


def snapshot_now(name: str, tasks: tuple[str, ...] = (), reason: str = "cleanup") -> list[Listed] | None:
    """Read a scoped queue on interruption, while the async controller is winding down."""
    calllog.record("squeue", reason)
    try:
        done = subprocess.run(_snapshot_args(name, tasks), capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return _listing(done.stdout) if done.returncode == 0 else None


async def accounting(tasks: list[str], *, intents: dict[str, str] | None = None) -> dict[str, str] | None:
    """Accounting states, optionally requiring the stored submission comment to prove identity."""
    if not tasks:
        return {}
    calllog.record("sacct", "classify")
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
            # As `CANCELLED by 2000`.
            found[task] = state.split()[0] if state.split() else state
    return found


def cancel(tasks: list[str]) -> list[str]:
    """Ask Slurm to cancel tasks, waiting for scancel but not for the tasks to end; the tasks it may not have
    cancelled, as scancel failed or did not answer. Used as a run stops, so it does not await."""
    failed = []
    for start in range(0, len(tasks), 500):
        chunk = tasks[start:start + 500]
        calllog.record("scancel", "cancel")
        try:
            # Name filters can widen a task ID to its whole array.
            done = subprocess.run(["scancel", *chunk], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                  timeout=10, env=_cancel_environment())
        except (OSError, subprocess.TimeoutExpired):
            failed.extend(chunk)
            continue
        if done.returncode != 0:
            failed.extend(chunk)
    return failed


@dataclass(frozen=True)
class Config:
    """What the site's configuration says that a run must keep within."""

    # An array's indices must be under it.
    max_array: int = 1001
    # How long slurmctld keeps an ended task, which `snapshot` shows only until then.
    min_job_age: float = 300.0


def cancel_name(name: str) -> bool:
    """Request cancellation of the disposable jobs under a unique probe name."""
    calllog.record("scancel", "probe")
    try:
        done = subprocess.run(["scancel", "-u", getpass.getuser(), f"--name={name}"], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=10, env=_cancel_environment())
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.returncode == 0


def _cancel_environment() -> dict[str, str]:
    # scancel also accepts filters and signal choices through its environment.
    return {key: value for key, value in os.environ.items() if not key.startswith("SCANCEL_")}


def site_values() -> dict[str, str] | None:
    """Each setting `scontrol show config` gives, by name, with its unit dropped; None when scontrol does not answer."""
    calllog.record("scontrol", "config")
    try:
        done = subprocess.run(["scontrol", "show", "config"], capture_output=True, text=True, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return _site_values(done.stdout)


async def check_site_values() -> dict[str, str] | None:
    """Get Slurm's config with a subprocess that can be stopped at the site-check deadline."""
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
    """The site's MaxArraySize and MinJobAge, in one `scontrol` call; Slurm's defaults where it cannot say."""
    values = site_values() or {}
    found = Config()
    array = values.get("MaxArraySize", "")
    age = values.get("MinJobAge", "")
    return Config(int(array) if array.isdigit() else found.max_array,
                  float(age) if age.isdigit() else found.min_job_age)
