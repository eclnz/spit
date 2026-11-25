from __future__ import annotations

import asyncio
from collections import deque
from datetime import datetime
from pathlib import Path
from typing import IO

LINE_LIMIT = 64 * 1024
DRAIN_SECONDS = 1.0
LOG_TAIL_LINES = 20


def now() -> str:
    return datetime.now().astimezone().isoformat(timespec="milliseconds")


class StampedLog:
    def __init__(self, path: Path):
        self.file: IO[bytes] = path.open("wb")

    def __enter__(self) -> StampedLog:
        return self

    def __exit__(self, *exc: object) -> None:
        self.file.close()

    def write(self, data: bytes) -> int:
        stamp = f"{now()} ".encode()
        self.file.write(b"".join(stamp + line for line in data.splitlines(keepends=True)))
        return len(data)

    def flush(self) -> None:
        self.file.flush()


def log_tail(path: Path, lines: int = LOG_TAIL_LINES) -> list[str]:
    try:
        with path.open(encoding="utf-8", errors="replace") as stream:
            return list(deque((line.rstrip("\n") for line in stream), lines))
    except OSError:
        return []


def open_log(path: Path, timestamps: bool) -> IO[bytes] | StampedLog:
    return StampedLog(path) if timestamps else path.open("wb")


async def pump(reader: asyncio.StreamReader, log: StampedLog) -> None:
    held = b""
    while chunk := await reader.read(LINE_LIMIT):
        held += chunk
        whole, newline, held = held.rpartition(b"\n")
        if newline:
            log.write(whole + newline)
        if len(held) >= LINE_LIMIT:
            log.write(held + b"\n")
            held = b""
        log.flush()
    if held:
        log.write(held + b"\n")
        log.flush()


async def drain(pumps: list[asyncio.Task[None]]) -> None:
    if not pumps:
        return
    _, waiting = await asyncio.wait(pumps, timeout=DRAIN_SECONDS)
    for task in waiting:
        task.cancel()
    await asyncio.gather(*pumps, return_exceptions=True)
