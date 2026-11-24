from __future__ import annotations

from ..files import Stamps, stored_each
from ..model import Job
from ..state.record import Record


def after_run(
    job: Job,
    inputs: Stamps,
    outputs: Stamps,
    input_hashes: dict[str, str] | None,
    output_hashes: dict[str, str] | None,
    seconds: float,
    run: str,
    attempt: int | None = None,
    memory: int | None = None,
    time_limit: float | None = None,
) -> Record:
    return Record(
        fingerprint=job.work.fingerprint,
        parts=job.work.parts,
        inputs=stored_each(job.keyed(inputs)),
        outputs=stored_each(job.keyed(outputs)),
        checks=job.work.checks_key,
        input_hashes=input_hashes,
        output_hashes=output_hashes,
        seconds=round(seconds, 3),
        output_paths=job.output_paths_by_slot,
        run=run,
        attempt=attempt,
        memory=memory,
        time_limit=time_limit,
    )


def adopted(job: Job, inputs: Stamps, outputs: Stamps) -> Record:
    return Record(
        fingerprint=job.work.fingerprint,
        parts=job.work.parts,
        inputs=stored_each(job.keyed(inputs)),
        outputs=stored_each(job.keyed(outputs)),
        output_paths=job.output_paths_by_slot,
    )


def refreshed(record: Record, job: Job, now: Stamps) -> Record | None:
    inputs = stored_each(job.keyed({path: now[path] for path in job.input_paths}))
    outputs = stored_each(job.keyed({path: now[path] for path in job.output_paths}))
    paths = job.output_paths_by_slot
    if (inputs, outputs, paths) == (record.inputs, record.outputs, record.output_paths):
        return None
    return record.model_copy(update={"inputs": inputs, "outputs": outputs, "output_paths": paths})


def checked(record: Record, job: Job) -> Record:
    return record.model_copy(update={"checks": job.work.checks_key})


def relocated(record: Record, job: Job) -> Record:
    return record.model_copy(update={"output_paths": job.output_paths_by_slot})
