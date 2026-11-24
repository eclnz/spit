"""`spit-bash job`: run one task of an intent on the node Slurm gave it, and write its result file."""

from __future__ import annotations

import asyncio
import os
import pickle
import random
import signal
import sys
import time
from pathlib import Path

from ..execute.executor import Finished, hash_both
from ..execute.failure import InputNotVisible
from ..execute.job import Failed, check_job, run_job
from ..files import stamp, stored
from ..model import Dag
from ..reuse.evidence import Compare
from . import batch as batches
from . import claim
from . import result as results
from .batch import Batch, Mode
from .result import Outcome, Result, host, write

# How soon a task looks again at inputs its node does not yet see as recorded: at first, and at most, each look
# waiting half as long again as the one before, jittered so an array's tasks do not look together.
VISIBLE_FIRST_SECONDS = 1.0
VISIBLE_MOST_SECONDS = 15.0


async def visible(dag: Dag, expected: tuple[tuple[str, tuple[int, ...]], ...], bound: float) -> InputNotVisible | None:
    """Wait until this node sees each input as the job that made it recorded it.

    A shared filesystem shows a file one node wrote on another within `bound` seconds of the write; the input's
    recorded time is when it was written. So this waits at most until the newest input is `bound` old. An input still
    unseen then never will be as recorded, and the job fails rather than read it. Opening a file makes an NFS client
    check it with the server, so each unseen input is opened, not only looked at.
    """
    if not expected:
        return None
    deadline = max(wanted[1] for _, wanted in expected) / 1e9 + bound
    pause = VISIBLE_FIRST_SECONDS
    while True:
        unseen = []
        for path, wanted in expected:
            absolute = dag.path(path)
            try:
                if path not in dag.folders:
                    with open(absolute, "rb"):
                        pass
            except OSError:
                pass
            seen = stored(stamp(absolute, path in dag.folders))
            if seen != list(wanted):
                unseen.append((path, wanted, None if seen is None else tuple(seen)))
        if not unseen:
            return None
        if time.time() >= deadline:
            return InputNotVisible(tuple(unseen), bound)
        await asyncio.sleep(min(pause, max(deadline - time.time(), 0.0)) * random.uniform(0.75, 1.25))
        pause = min(pause * 1.5, VISIBLE_MOST_SECONDS)


async def perform(batch: Batch, index: int, task: str) -> Result:
    """Run the task's job as a local run would, hash its files here, and say how it ended.

    It runs only while it holds the job's claim, which it takes once its inputs are seen and before it touches an
    output. SIGTERM, which Slurm sends at the time limit and on `scancel`, stops the job's commands and puts back the
    earlier outputs, as cancelling a local run does, and the result says it was terminated.
    """
    spec = batch.tasks[index]
    dag, options = batch.dag, batch.options
    job = batch.job(spec)
    spec.log.parent.mkdir(parents=True, exist_ok=True)

    def said(outcome: Outcome, finished: Finished | None = None, failure=None) -> Result:
        return Result(task, batch.intent, index, options.run.name, job.id, job.work.key, job.work.fingerprint,
                      outcome, host(), finished, failure, spec.attempt, spec.memory, spec.time_limit)

    loop = asyncio.get_running_loop()
    main = asyncio.current_task()
    assert main is not None, "perform runs as a task"
    loop.add_signal_handler(signal.SIGTERM, main.cancel)
    try:
        hidden = await visible(dag, spec.expected, batch.visibility)
        if hidden is not None:
            return said(Outcome.FAILED, failure=hidden)
        if not claim.take(batch.folder, batch.intent, index, task):
            return said(Outcome.BUSY)
        if spec.mode is Mode.CHECK:
            failure = await check_job(dag, job, spec.log, options)
            return said(Outcome.CHECKED) if failure is None else said(Outcome.FAILED, failure=failure)
        ran = await run_job(dag, job, spec.log, options, reason=spec.reason, keep_edited=spec.keep_edited)
        if isinstance(ran, Failed):
            return said(Outcome.FAILED, failure=ran.failure)
        if options.compare is Compare.STAT:
            return said(Outcome.MADE, Finished(ran, None, None))
        # Here, on the node that wrote the files, rather than on the controller over the shared filesystem.
        return said(Outcome.MADE, Finished(ran, *hash_both(dag, job)))
    except asyncio.CancelledError:
        return said(Outcome.TERMINATED)
    finally:
        loop.remove_signal_handler(signal.SIGTERM)


def main(path: str, index: str) -> int:
    """Run task `index` of the intent whose batch is at `path`; 0 once its result is written, whatever the job did."""
    try:
        batch = batches.read(Path(path))
        number = int(index)
        batch.tasks[number]
    except (OSError, ValueError, IndexError, EOFError, pickle.UnpicklingError, AttributeError, ImportError) as exc:
        print(f"spit-bash job: cannot read task {index} of {path}: {exc}", file=sys.stderr)
        return 2
    # The task's Slurm id names its result, so attempts and twins never share one.
    task = f"{os.environ.get('SLURM_ARRAY_JOB_ID', os.environ.get('SLURM_JOB_ID', '0'))}_{number}"
    result = asyncio.run(perform(batch, number, task))
    try:
        write(result, results.path(batch.folder, task))
    except OSError as exc:
        print(f"spit-bash job: cannot write the result of {task}: {exc}", file=sys.stderr)
        return 2
    return 0
