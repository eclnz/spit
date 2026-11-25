from __future__ import annotations

from typing import Annotated

from pydantic import BaseModel, ConfigDict, Field

FileStamp = Annotated[list[int], Field(min_length=2, max_length=3)]


class Record(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    fingerprint: str
    parts: dict[str, str] | None = None
    inputs: dict[str, FileStamp]
    outputs: dict[str, FileStamp]
    checks: str | None = None
    input_hashes: dict[str, str] | None = None
    output_hashes: dict[str, str] | None = None
    seconds: float | None = None
    output_paths: dict[str, str] | None = None
    run: str | None = None
    attempt: int | None = None
    memory: int | None = None
    time_limit: float | None = None
