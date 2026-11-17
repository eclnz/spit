"""The Slurm commands the controller runs, and what it reads from their output.

Each call has a timeout, as a busy `slurmctld` may not answer. The run reads the queue in one `squeue` call for all
its tasks, filtered here by the run's name prefix, and asks `sacct` only about tasks that left the queue without a
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
class Queued:
    """A task as `squeue` lists it."""

    task: str
    name: str
    state: str
    reason: str
    comment: str


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


async def queue(prefix: str, names: tuple[str, ...] = ()) -> dict[str, Queued] | None:
    """Every task of this user whose name starts with `prefix`, by task id as `123_4`; None when squeue fails.

    With `names`, slurmctld lists only jobs of those names, rather than every job of the user's.
    """
    named = [f"--name={','.join(names)}"] if names else []
    try:
        code, out, _ = await call(["squeue", "-u", getpass.getuser(), *named, "-h", "-r", "-o", "%i|%j|%T|%r|%k"])
    except SlurmError:
        return None
    if code != 0:
        return None
    found = {}
    for line in out.splitlines():
        fields = line.strip().split("|")
        if len(fields) >= 5 and fields[1].startswith(prefix):
            found[fields[0]] = Queued(fields[0], fields[1], fields[2], fields[3], "|".join(fields[4:]))
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


def max_array_size() -> int | None:
    """The site's MaxArraySize: an array's indices must be under it. None when scontrol cannot say."""
    try:
        done = subprocess.run(["scontrol", "show", "config"], capture_output=True, text=True, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in done.stdout.splitlines():
        name, _, value = line.partition("=")
        if name.strip() == "MaxArraySize" and value.strip().isdigit():
            return int(value.strip())
    return None
