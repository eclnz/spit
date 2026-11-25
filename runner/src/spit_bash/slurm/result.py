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

VERSION = 2


class Outcome(Enum):
    MADE = "made"
    CHECKED = "checked"
    FAILED = "failed"
    TERMINATED = "terminated"
    BUSY = "busy"


@dataclass(frozen=True)
class Result:
    task: str
    intent: str
    index: int
    run: str
    job: int
    key: str
    fingerprint: str
    outcome: Outcome
    host: str
    finished: Finished | None = None
    failure: Failure | None = None


def write(result: Result, path: Path) -> None:
    document: dict[str, Any] = {
        "version": VERSION,
        "task": result.task,
        "intent": result.intent,
        "index": result.index,
        "run": result.run,
        "job": result.job,
        "key": result.key,
        "fingerprint": result.fingerprint,
        "outcome": result.outcome.value,
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
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    except OSError:
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
        return Result(
            task=document["task"],
            intent=document["intent"],
            index=document["index"],
            run=document["run"],
            job=document["job"],
            key=document["key"],
            fingerprint=document["fingerprint"],
            outcome=Outcome(document["outcome"]),
            host=document["host"],
            finished=finished,
            failure=decode(document["failure"]) if "failure" in document else None,
        )
    except (KeyError, TypeError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read the result {path}: {exc}") from exc


def host() -> str:
    return socket.gethostname()


def folder(slurm: Path) -> Path:
    return slurm / "results"


def path(slurm: Path, task: str) -> Path:
    return folder(slurm) / f"{task}.json"
