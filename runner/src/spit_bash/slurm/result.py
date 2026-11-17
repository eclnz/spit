"""The result file a task writes once its job has ended, and the controller reads: the only thing a job on a node
tells the controller, which alone writes the state."""

from __future__ import annotations

import json
import os
import socket
import tempfile
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

from ..execute.executor import Finished
from ..execute.failure import Failure
from ..execute.job import Made
from .codec import decode, encode

# The version of the result file this spit-bash writes and reads; a result of another version is not read.
VERSION = 1


class Outcome(Enum):
    """How a task's job ended."""

    # Its commands succeeded, and its files were hashed.
    MADE = "made"
    # Its checks alone passed.
    CHECKED = "checked"
    # It failed, as `failure` says.
    FAILED = "failed"
    # Slurm stopped it, at its time limit or by `scancel`, after it put back the earlier outputs.
    TERMINATED = "terminated"


@dataclass(frozen=True)
class Result:
    """What one task says of its job."""

    token: str
    job: int
    key: str
    fingerprint: str
    attempt: int
    outcome: Outcome
    host: str
    # For `Outcome.MADE`.
    finished: Finished | None = None
    # For `Outcome.FAILED`.
    failure: Failure | None = None


def write(result: Result, path: Path) -> None:
    """Write `result` so the controller sees all of it or none: to a temporary name, synced, then renamed."""
    document: dict[str, Any] = {
        "version": VERSION, "token": result.token, "job": result.job, "key": result.key,
        "fingerprint": result.fingerprint, "attempt": result.attempt, "outcome": result.outcome.value,
        "host": result.host,
    }
    if result.finished is not None:
        document["made"] = encode(result.finished.made)
        document["input_hashes"] = result.finished.input_hashes
        document["output_hashes"] = result.finished.output_hashes
    if result.failure is not None:
        document["failure"] = encode(result.failure)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(document, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read(path: Path) -> Result | None:
    """The result at `path`, or None when there is none yet. Raises `ValueError` for one this spit-bash cannot read."""
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    except OSError:
        # As a stale handle on a shared filesystem: read it again later. A task that left the queue and whose
        # result never reads is given up on after `result_wait`.
        return None
    try:
        document = json.loads(text)
        if document.get("version") != VERSION:
            raise ValueError(f"result version {document.get('version')!r}, where this spit-bash reads {VERSION}")
        finished = None
        if "made" in document:
            made = decode(document["made"])
            if not isinstance(made, Made):
                raise ValueError("`made` is not a job's outputs")
            finished = Finished(made, document["input_hashes"], document["output_hashes"])
        return Result(token=document["token"], job=document["job"], key=document["key"],
                      fingerprint=document["fingerprint"], attempt=document["attempt"],
                      outcome=Outcome(document["outcome"]), host=document["host"], finished=finished,
                      failure=decode(document["failure"]) if "failure" in document else None)
    except (KeyError, TypeError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read the result {path}: {exc}") from exc


def host() -> str:
    return socket.gethostname()
