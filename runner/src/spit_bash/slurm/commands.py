"""The Slurm commands the controller runs, and what it reads from their output.

Each call has a timeout, as a busy `slurmctld` may not answer. The run reads the queue in one `squeue` call for all
its tasks, filtered by the runner folder's job name, and asks `sacct` only about tasks that left the queue without a
result. `sacct` is optional: with accounting off or its database down, the run goes on without the reasons it gives.
"""

from __future__ import annotations

import asyncio
import getpass
import subprocess
from dataclasses import dataclass
from pathlib import Path

# How long one call may take before the controller gives up on it.
TIMEOUT = 60.0

# What sbatch says when the user's queue or the site's limits are full: wait and submit again.
BUSY = ("MaxSubmitJob", "QOSMaxSubmitJob", "AssocMaxSubmitJob", "Job violates accounting/QOS policy",
        "Resource temporarily unavailable", "Slurm temporarily unable to accept job")
# What sbatch says when it could not tell whether the job was accepted: look for it before submitting again.
UNSURE = ("Socket timed out", "Unable to contact slurm controller", "Connection refused", "timed out")


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
        process.kill()
        await process.wait()
        raise Unsure(f"{args[0]} did not answer within {timeout:.0f}s") from None
    except asyncio.CancelledError:
        # A run being stopped: the call is not left running, unseen, to submit or cancel after the run has gone.
        process.kill()
        await process.wait()
        raise
    assert process.returncode is not None, "a process that was waited for has a status"
    return process.returncode, out.decode(errors="replace"), err.decode(errors="replace")


async def submit(script: Path, options: list[str]) -> str:
    """Submit an array; its Slurm job id. Raises `Busy`, `Unsure` or `SlurmError`."""
    code, out, err = await call(["sbatch", "--parsable", *options, str(script)])
    if code == 0 and out.strip():
        # `--parsable` prints the id, and `;cluster` on a multi-cluster site.
        return out.strip().splitlines()[-1].split(";")[0]
    words = err.strip() or out.strip() or f"exited with status {code}"
    if any(mark in words for mark in BUSY):
        raise Busy(words)
    if any(mark in words for mark in UNSURE):
        raise Unsure(words)
    raise SlurmError(f"sbatch failed: {words}")


async def snapshot(name: str) -> list[Listed] | None:
    """Every task of this user's named `name`, queued, running or ended within MinJobAge, in one call; None when
    squeue fails.

    slurmctld filters by the name, so the call lists the runner folder's tasks only, whatever pipeline submitted them.
    """
    try:
        code, out, _ = await call(["squeue", "-u", getpass.getuser(), f"--name={name}", "-r", "-t", "all", "-h",
                                   "-o", "%i|%K|%T|%o"])
    except SlurmError:
        return None
    if code != 0:
        return None
    found = []
    for line in out.splitlines():
        task, _, rest = line.strip().partition("|")
        index, _, rest = rest.partition("|")
        state, _, command = rest.partition("|")
        if task:
            found.append(Listed(task, int(index) if index.isdigit() else 0, state.split()[0] if state else "", command))
    return found


async def accounting(tasks: list[str]) -> dict[str, str] | None:
    """Each task's state as sacct gives it, as `TIMEOUT` or `OUT_OF_MEMORY`; None when sacct fails or is off."""
    if not tasks:
        return {}
    try:
        code, out, _ = await call(["sacct", "-X", "-n", "-P", "-j", ",".join(tasks), "--format=JobID,State"])
    except SlurmError:
        return None
    if code != 0:
        return None
    found = {}
    for line in out.splitlines():
        task, _, state = line.strip().partition("|")
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
        try:
            done = subprocess.run(["scancel", *chunk], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                  timeout=TIMEOUT)
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
    """Cancel every task of this user's named `name`, in one call; whether scancel succeeded. Used as a run stops."""
    try:
        done = subprocess.run(["scancel", "-u", getpass.getuser(), f"--name={name}"], stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.returncode == 0


def site_values() -> dict[str, str] | None:
    """Each setting `scontrol show config` gives, by name, with its unit dropped; None when scontrol does not answer."""
    try:
        done = subprocess.run(["scontrol", "show", "config"], capture_output=True, text=True, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    values = {}
    for line in done.stdout.splitlines():
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
