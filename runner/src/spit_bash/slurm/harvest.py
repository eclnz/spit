"""What a controller does first with the tasks an earlier one left pending in the ledger.

A result an earlier controller did not read is recorded, so a job that finished while no controller ran is not run
again. A task still in the queue is taken over by this run, never submitted twice. A task that left the queue
without a result is waited for a while, as a shared filesystem may show the result late, then given up on.
"""

from __future__ import annotations

import asyncio
import os
import time
from dataclasses import dataclass, field
from pathlib import Path

from ..model import Dag, DagError, Job
from ..reuse import records
from ..state.store import StateWrite, Store
from . import commands
from .ledger import End, Entry, Ledger
from .result import Outcome, read


@dataclass
class Harvest:
    """What the ledger's pending tasks came to."""

    # Successes recorded in the state, and results of failed jobs read.
    recorded: int = 0
    failed: int = 0
    # Tasks still queued or running, by job key: the newest for each job.
    live: dict[str, Entry] = field(default_factory=dict)
    # Tasks that left the queue and showed no result in time.
    lost: int = 0
    # Tasks of the run's prefix in the queue that no ledger entry held, which were cancelled.
    strays: int = 0

    def lines(self) -> list[str]:
        """What a run says of it before planning."""
        said = []
        if self.recorded or self.failed:
            said.append(f"read {self.recorded + self.failed} Slurm result{'' if self.recorded + self.failed == 1 else 's'}"
                        f" an earlier run left: {self.recorded} succeeded, {self.failed} failed")
        if self.live:
            said.append(f"{len(self.live)} Slurm task{'' if len(self.live) == 1 else 's'} an earlier run submitted "
                        f"{'is' if len(self.live) == 1 else 'are'} still queued or running")
        if self.strays:
            said.append(f"cancelled {self.strays} Slurm task{'' if self.strays == 1 else 's'} of this folder's that no "
                        "run had recorded, as an earlier controller killed while submitting leaves")
        if self.lost:
            said.append(f"{self.lost} Slurm task{'' if self.lost == 1 else 's'} an earlier run submitted ended with no "
                        "result; their jobs are planned again")
        return said


def _take(jobs: dict[str, Job], state: Store, entry: Entry, write: bool, found: Harvest) -> bool:
    """Record the task's result, if it has one; whether it had."""
    try:
        result = read(Path(entry.result))
    except ValueError:
        result = None
        if write:
            found.lost += 1
        return True
    if result is None:
        return False
    if write:
        # Recorded in the state, or of a job that failed: the results folder holds only results still to read.
        Path(entry.result).unlink(missing_ok=True)
    job = jobs.get(entry.key)
    if (result.outcome is Outcome.MADE and result.finished is not None and job is not None
            and job.work.fingerprint == entry.fingerprint == result.fingerprint):
        made = result.finished.made
        record = records.after_run(job, made.inputs, made.outputs, result.finished.input_hashes,
                                   result.finished.output_hashes, made.seconds, entry.run)
        if write:
            state.record(entry.key, record)
        else:
            state.assume((StateWrite(entry.key, record),))
        found.recorded += 1
    else:
        found.failed += 1
    return True


def _present(entries: list[Entry]) -> list[Entry]:
    """The entries whose result file is there, from one listing of each results folder, not one open per entry."""
    names: dict[Path, set[str]] = {}
    found = []
    for entry in entries:
        result = Path(entry.result)
        if result.parent not in names:
            try:
                names[result.parent] = set(os.listdir(result.parent))
            except OSError:
                names[result.parent] = set()
        if result.name in names[result.parent]:
            found.append(entry)
    return found


# The states in which sacct says a task ended for good without writing a result, so none is coming.
DEAD = ("BOOT_FAIL", "CANCELLED", "DEADLINE", "FAILED", "NODE_FAIL", "OUT_OF_MEMORY", "PREEMPTED", "TIMEOUT")


async def harvest(dag: Dag, state: Store, ledger: Ledger, prefix: str, wait: float, write: bool,
                  names: tuple[str, ...] = ()) -> Harvest:
    """Deal with every pending task of `ledger`. Without `write`, as for `plan`, record nothing, cancel nothing and
    wait for nothing.

    A run also cancels every task of `prefix` in the queue that the ledger does not hold: one submitted by a
    controller killed before it could record it, or one sbatch accepted after saying it had failed.
    """
    found = Harvest()
    jobs = {job.work.key: job for job in dag.jobs}
    unread = []
    entries = sorted(ledger.pending.values(), key=lambda entry: entry.at)
    present = {entry.token for entry in _present(entries)}
    for entry in entries:
        if entry.token in present and _take(jobs, state, entry, write, found):
            if write:
                ledger.ended(entry.token, End.READ)
        else:
            unread.append(entry)
    if not unread and not write:
        return found
    queued = await commands.queue(prefix, names)
    if queued is None:
        if write and unread:
            raise DagError(f"cannot read the Slurm queue to check {len(unread)} task(s) an earlier run submitted; "
                           "run again once squeue answers")
        return found
    if write:
        # Every task the ledger held, those just read included, which may linger in the queue as they end.
        known = {entry.task for entry in entries}
        unknown = sorted(task for task in queued if task not in known)
        if unknown:
            commands.cancel(unknown)
            found.strays = len(unknown)
    left = []
    for entry in unread:
        if entry.task in queued:
            earlier = found.live.get(entry.key)
            if earlier is not None and write:
                # An older attempt of the same job, which the newer one replaced.
                commands.cancel([earlier.task])
                ledger.ended(earlier.token, End.CANCELLED)
            found.live[entry.key] = entry
        else:
            left.append(entry)
    if not write or not left:
        return found
    # A task sacct says died has no result coming; one it says completed, or that it cannot say of, may have a
    # result the shared filesystem shows late.
    states = await commands.accounting([entry.task for entry in left]) or {}
    waiting = []
    for entry in left:
        if states.get(entry.task) in DEAD:
            found.lost += 1
            ledger.ended(entry.token, End.LOST)
        else:
            waiting.append(entry)
    deadline = time.monotonic() + wait
    while waiting and time.monotonic() < deadline:
        await asyncio.sleep(min(5.0, wait))
        present = {entry.token for entry in _present(waiting)}
        still = []
        for entry in waiting:
            if entry.token in present and _take(jobs, state, entry, write, found):
                ledger.ended(entry.token, End.READ)
            else:
                still.append(entry)
        waiting = still
    for entry in waiting:
        found.lost += 1
        ledger.ended(entry.token, End.LOST)
    return found
