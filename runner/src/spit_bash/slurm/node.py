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

VISIBLE_FIRST_SECONDS = 1.0
VISIBLE_MOST_SECONDS = 15.0


async def visible(dag: Dag, expected: tuple[tuple[str, tuple[int, ...]], ...], bound: float) -> InputNotVisible | None:
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
    spec = batch.tasks[index]
    dag, options = batch.dag, batch.options
    job = batch.job(spec)
    spec.log.parent.mkdir(parents=True, exist_ok=True)

    def said(outcome: Outcome, finished: Finished | None = None, failure=None) -> Result:
        return Result(
            task,
            batch.intent,
            index,
            options.run.name,
            job.id,
            job.work.key,
            job.work.fingerprint,
            outcome,
            host(),
            finished,
            failure,
            spec.attempt,
            spec.memory,
            spec.time_limit,
        )

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
        return said(Outcome.MADE, Finished(ran, *hash_both(dag, job)))
    except asyncio.CancelledError:
        return said(Outcome.TERMINATED)
    finally:
        loop.remove_signal_handler(signal.SIGTERM)


def main(path: str, index: str) -> int:
    try:
        batch = batches.read(Path(path))
        number = int(index)
        batch.tasks[number]
    except (OSError, ValueError, IndexError, EOFError, pickle.UnpicklingError, AttributeError, ImportError) as exc:
        print(f"spit-bash job: cannot read task {index} of {path}: {exc}", file=sys.stderr)
        return 2
    task = f"{os.environ.get('SLURM_ARRAY_JOB_ID', os.environ.get('SLURM_JOB_ID', '0'))}_{number}"
    result = asyncio.run(perform(batch, number, task))
    try:
        write(result, results.path(batch.folder, task))
    except OSError as exc:
        print(f"spit-bash job: cannot write the result of {task}: {exc}", file=sys.stderr)
        return 2
    return 0
