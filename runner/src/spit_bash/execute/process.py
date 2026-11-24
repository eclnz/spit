from __future__ import annotations

import asyncio
import os
import shlex
import signal
from contextlib import ExitStack
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Union

from ..logs import StampedLog, drain, pump
from ..memory import group_memory
from ..model import Dag, Layout

STOP_GRACE_SECONDS = 5.0
MEMORY_POLL_SECONDS = 0.5


@dataclass(frozen=True)
class Exited:
    code: int


@dataclass(frozen=True)
class KilledMemory:
    used: int
    limit: int


@dataclass(frozen=True)
class KilledSignal:
    limit: int | None


@dataclass(frozen=True)
class NotStarted:
    error: str


CommandEnd = Union[Exited, KilledMemory, KilledSignal, NotStarted]


def succeeded(end: CommandEnd) -> bool:
    return isinstance(end, Exited) and end.code == 0


def partial(dag: Dag, stdout: str) -> Path:
    return Layout.partial(dag.checked(stdout))


async def run_command(dag: Dag, args: tuple[str, ...], log: IO[bytes] | StampedLog,
                      stdin: str | None = None, stdout: str | None = None, limit: int | None = None, *, stderr: IO[bytes] | None = None) -> CommandEnd:
    shown = shlex.join(args)
    if stdin is not None:
        shown += f" < {shlex.quote(stdin)}"
    if stdout is not None:
        shown += f" > {shlex.quote(stdout)}"
    log.write(f"$ {shown}\n".encode())
    log.flush()
    with ExitStack() as files:
        try:
            source = asyncio.subprocess.DEVNULL if stdin is None else files.enter_context(dag.checked(stdin).open("rb"))
            stamped = isinstance(log, StampedLog)
            piped = asyncio.subprocess.PIPE
            target = (piped if stamped else log) if stdout is None else files.enter_context(partial(dag, stdout).open("wb"))
            process = await asyncio.create_subprocess_exec(
                *args, cwd=dag.outputs, stdin=source, stdout=target, stderr=stderr if stderr is not None else piped if stamped else log,
                start_new_session=True,
            )
        except OSError as exc:
            return NotStarted(str(exc))
    pumps = [asyncio.ensure_future(pump(stream, log)) for stream in (process.stdout, process.stderr)
             if stream is not None and isinstance(log, StampedLog)]
    try:
        return await _finish(process, pumps, limit)
    except asyncio.CancelledError:
        for task in pumps:
            task.cancel()
        raise


async def _finish(process: asyncio.subprocess.Process, pumps: list[asyncio.Task[None]],
                  limit: int | None) -> CommandEnd:
    try:
        if limit is None:
            code = await process.wait()
        else:
            used = await _within(process, limit)
            if used is not None:
                await _kill(process)
                await drain(pumps)
                return KilledMemory(used, limit)
            code = process.returncode
    except asyncio.CancelledError:
        await _stop(process)
        raise
    await drain(pumps)
    if code == -signal.SIGKILL:
        return KilledSignal(limit)
    assert code is not None, "a command that was waited for has a return code"
    return Exited(code)


async def _within(process: asyncio.subprocess.Process, limit: int) -> int | None:
    while True:
        try:
            await asyncio.wait_for(asyncio.shield(process.wait()), MEMORY_POLL_SECONDS)
            return None
        except asyncio.TimeoutError:
            used = await asyncio.to_thread(group_memory, process.pid)
            if used > limit:
                return used


async def _kill(process: asyncio.subprocess.Process) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    await process.wait()


async def _stop(process: asyncio.subprocess.Process) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        await asyncio.wait_for(process.wait(), STOP_GRACE_SECONDS)
    except asyncio.TimeoutError:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    await process.wait()
