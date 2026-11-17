"""`spit-bash job`: run one task of a batch on the node Slurm gave it, and write its result file."""

from __future__ import annotations

import asyncio
import pickle
import signal
import sys
from pathlib import Path

from ..execute.executor import Finished, hash_both
from ..execute.job import Failed, check_job, run_job
from ..reuse.evidence import Compare
from . import batch as batches
from .batch import Batch, Mode, Spec
from .result import Outcome, Result, host, write


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
