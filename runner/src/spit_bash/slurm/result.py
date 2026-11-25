from __future__ import annotations

import json
import socket
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import cast, get_args

from pydantic import BaseModel, ConfigDict, Field

from ..execute.executor import Finished
from ..execute.failure import Failure
from ..execute.job import Made
from . import atomic
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
    attempt: int | None = None
    memory: int | None = None
    time_limit: float | None = None


class ResultFile(BaseModel):
    model_config = ConfigDict(strict=True)

    version: int
    task: str = Field(pattern=r"^[0-9]+(?:_[0-9]+)?$")
    intent: str
    index: int = Field(ge=0)
    run: str
    job: int
    key: str
    fingerprint: str
    outcome: str
    host: str
    made: object = None
    input_hashes: dict[str, str] | None = None
    output_hashes: dict[str, str] | None = None
    failure: object = None
    attempt: int | None = None
    memory: int | None = None
    time_limit: float | None = None

    def result(self) -> Result:
        if self.version != VERSION:
            raise ValueError(f"result version {self.version!r}, where this spit-bash reads {VERSION}")
        outcome = Outcome(self.outcome)
        finished = self._finished(outcome)
        failure = self._failure(outcome)
        return Result(
            task=self.task,
            intent=self.intent,
            index=self.index,
            run=self.run,
            job=self.job,
            key=self.key,
            fingerprint=self.fingerprint,
            outcome=outcome,
            host=self.host,
            finished=finished,
            failure=failure,
            attempt=self.attempt,
            memory=self.memory,
            time_limit=self.time_limit,
        )

    def _finished(self, outcome: Outcome) -> Finished | None:
        fields = {"made", "input_hashes", "output_hashes"}
        present = fields & self.model_fields_set
        if outcome is not Outcome.MADE:
            if present:
                raise ValueError(f"a {outcome.value} result cannot contain outputs")
            return None
        if present != fields:
            raise ValueError("a made result needs outputs and both hash fields")
        made = decode(self.made)
        if not isinstance(made, Made):
            raise ValueError("`made` is not a job's outputs")
        if any(stamp is None for stamp in (*made.inputs.values(), *made.outputs.values())):
            raise ValueError("a made result needs stamps for every artifact")
        return Finished(made, self.input_hashes, self.output_hashes)

    def _failure(self, outcome: Outcome) -> Failure | None:
        if outcome is not Outcome.FAILED:
            if "failure" in self.model_fields_set:
                raise ValueError(f"a {outcome.value} result cannot contain a failure")
            return None
        failure = decode(self.failure)
        if not isinstance(failure, get_args(Failure)):
            raise ValueError("a failed result needs a job failure")
        return cast(Failure, failure)


def write(result: Result, path: Path) -> None:
    document: dict[str, object] = {
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
    for name in ("attempt", "memory", "time_limit"):
        if getattr(result, name) is not None:
            document[name] = getattr(result, name)
    atomic.write(path, json.dumps(document).encode())


def read(path: Path) -> Result | None:
    try:
        return ResultFile.model_validate_json(path.read_bytes()).result()
    except OSError:
        return None
    except (KeyError, TypeError, ValueError, RecursionError) as exc:
        raise ValueError(f"cannot read the result {path}: {exc}") from exc


def host() -> str:
    return socket.gethostname()


def folder(slurm: Path) -> Path:
    return slurm / "results"


def path(slurm: Path, task: str) -> Path:
    return folder(slurm) / f"{task}.json"
