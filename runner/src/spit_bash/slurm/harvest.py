"""What a controller does first with the tasks an earlier one left pending in the ledger.

A result an earlier controller did not read is recorded, so a job that finished while no controller ran is not run
again. A task still in the queue is taken over by this run, never submitted twice. A task that left the queue
without a result is waited for a while, as a shared filesystem may show the result late, then given up on.
"""

from __future__ import annotations

import asyncio
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

    def lines(self) -> list[str]:
        """What a run says of it before planning."""
        said = []
        if self.recorded or self.failed:
            said.append(f"read {self.recorded + self.failed} Slurm result{'' if self.recorded + self.failed == 1 else 's'}"
                        f" an earlier run left: {self.recorded} succeeded, {self.failed} failed")
        if self.live:
            said.append(f"{len(self.live)} Slurm task{'' if len(self.live) == 1 else 's'} an earlier run submitted "
                        f"{'is' if len(self.live) == 1 else 'are'} still queued or running")
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


async def harvest(dag: Dag, state: Store, ledger: Ledger, prefix: str, wait: float, write: bool) -> Harvest:
    """Deal with every pending task of `ledger`. Without `write`, as for `plan`, record nothing and wait for nothing."""
    found = Harvest()
    if not ledger.pending:
        return found
    jobs = {job.work.key: job for job in dag.jobs}
    unread = []
    for entry in sorted(ledger.pending.values(), key=lambda entry: entry.at):
        if _take(jobs, state, entry, write, found):
            if write:
                ledger.ended(entry.token, End.READ)
        else:
            unread.append(entry)
    if not unread:
        return found
    queued = await commands.queue(prefix)
    if queued is None:
        if write:
            raise DagError(f"cannot read the Slurm queue to check {len(unread)} task(s) an earlier run submitted; "
                           "run again once squeue answers")
        return found
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
    if not write:
        return found
    deadline = time.monotonic() + wait
    while left and time.monotonic() < deadline:
        await asyncio.sleep(min(2.0, wait))
        still = []
        for entry in left:
            if _take(jobs, state, entry, write, found):
                ledger.ended(entry.token, End.READ)
            else:
                still.append(entry)
        left = still
    for entry in left:
        found.lost += 1
        ledger.ended(entry.token, End.LOST)
    return found
