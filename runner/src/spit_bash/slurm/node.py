"""`spit-bash job`: run one task of a batch on the node Slurm gave it, and write its result file."""

from __future__ import annotations

import asyncio
import pickle
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
from .batch import Batch, Mode, Spec
from .result import Outcome, Result, host, write


# How often a task looks again at inputs its node does not yet see as their jobs recorded them.
VISIBLE_POLL_SECONDS = 1.0


async def visible(dag: Dag, expected: tuple[tuple[str, tuple[int, ...]], ...], wait: float) -> InputNotVisible | None:
    """Wait until this node sees each input as the job that made it recorded it, or `wait` seconds pass.

    A shared filesystem's client caches what it has seen, so a file a job on another node rewrote moments ago can
    show here, for a while, as it was before. A job started then would read the old file, or find its input changed
    while it ran. Opening a file makes an NFS client check it with the server, so each unseen input is opened, not
    only looked at.
    """
    started = time.monotonic()
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
        if time.monotonic() - started >= wait:
            return InputNotVisible(tuple(unseen), wait)
        await asyncio.sleep(VISIBLE_POLL_SECONDS)


async def perform(batch: Batch, spec: Spec) -> Result:
    """Run the task's job as a local run would, hash its files here, and say how it ended.

    SIGTERM, which Slurm sends before a time limit and on `scancel`, stops the
    job's commands and puts back the earlier outputs, as cancelling a local run
    does, and the result says it was terminated.
    """
    dag, options = batch.dag, batch.options
    job = batch.job(spec)
    spec.log.parent.mkdir(parents=True, exist_ok=True)

    def said(outcome: Outcome, finished: Finished | None = None, failure=None) -> Result:
        return Result(spec.token, job.id, job.work.key, job.work.fingerprint, spec.attempt, outcome, host(),
                      finished, failure)

    loop = asyncio.get_running_loop()
    main = asyncio.current_task()
    assert main is not None, "perform runs as a task"
    loop.add_signal_handler(signal.SIGTERM, main.cancel)
    try:
        hidden = await visible(dag, spec.expected, batch.visible_wait)
        if hidden is not None:
            return said(Outcome.FAILED, failure=hidden)
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
    """Run task `index` of the batch at `path`; 0 once its result is written, whatever the job's outcome."""
    try:
        batch = batches.read(Path(path))
        spec = batch.tasks[int(index)]
    except (OSError, ValueError, IndexError, EOFError, pickle.UnpicklingError, AttributeError, ImportError) as exc:
        print(f"spit-bash job: cannot read task {index} of {path}: {exc}", file=sys.stderr)
        return 2
    result = asyncio.run(perform(batch, spec))
    try:
        write(result, spec.result)
    except OSError as exc:
        print(f"spit-bash job: cannot write the result {spec.result}: {exc}", file=sys.stderr)
        return 2
    return 0
