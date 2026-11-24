from __future__ import annotations

import dataclasses
import json
import os
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from enum import Enum
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any

from ..schema import VERSION as DAG_VERSION
from .profile import Profile

POLL_SECONDS = 1.0
SACCT_TIMEOUT = 60.0
TASKS = 2
HOG_MEGABYTES = 400
ACCOUNTING_TRIES = 6
ACCOUNTING_PAUSE = 2.0


class Mode(str, Enum):
    CHECK = "check"
    HOG = "hog"
    SLEEP = "sleep"


class Claim(str, Enum):
    CREATED = "created"
    EXISTS = "exists"
    FAILED = "failed"


@dataclass(frozen=True)
class Attempt:
    outcome: Claim
    why: str = ""

    @staticmethod
    def of(path: Path) -> Attempt:
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
        except FileExistsError:
            return Attempt(Claim.EXISTS)
        except OSError as exc:
            return Attempt(Claim.FAILED, exc.strerror or str(exc))
        os.close(fd)
        return Attempt(Claim.CREATED)

    @staticmethod
    def from_json(data: dict[str, Any]) -> Attempt:
        return Attempt(Claim(data["outcome"]), data.get("why", ""))


@dataclass(frozen=True)
class Accounting:
    ok: bool
    why: str = ""


@dataclass(frozen=True)
class Identity:
    user: str
    uid: int
    spit_bash: str
    dag: int
    host: str

    @staticmethod
    def here() -> Identity:
        return Identity(user(), os.getuid(), release(), DAG_VERSION, socket.gethostname())


@dataclass(frozen=True)
class Found:
    index: int
    identity: Identity
    umask: int
    python: str
    job: str
    by_controller: Attempt
    by_node: Attempt
    contested: Attempt
    accounting: Accounting
    quiet: bool
    written: float

    def to_json(self) -> dict[str, Any]:
        return dataclasses.asdict(self)

    @staticmethod
    def from_json(data: dict[str, Any]) -> Found | None:
        try:
            return Found(**{**data, "identity": Identity(**data["identity"]),
                            "by_controller": Attempt.from_json(data["by_controller"]),
                            "by_node": Attempt.from_json(data["by_node"]),
                            "contested": Attempt.from_json(data["contested"]),
                            "accounting": Accounting(**data["accounting"])})
        except (KeyError, TypeError, ValueError):
            return None


@dataclass(frozen=True)
class Heard:
    index: int
    heard: bool
    seen: float
    written: float | None

    def to_json(self) -> dict[str, Any]:
        return dataclasses.asdict(self)

    @staticmethod
    def from_json(data: dict[str, Any]) -> Heard | None:
        try:
            return Heard(**data)
        except TypeError:
            return None


@dataclass(frozen=True)
class Paths:
    folder: Path

    def make(self) -> None:
        for each in ("exclusive", "up", "down", "term", "enforce"):
            (self.folder / each).mkdir(parents=True, exist_ok=True)

    @property
    def by_controller(self) -> Path:
        return self.folder / "exclusive" / "by-controller"

    def by_node(self, index: int) -> Path:
        return self.folder / "exclusive" / f"by-node-{index}"

    @property
    def contested(self) -> Path:
        return self.folder / "exclusive" / "contested"

    def found(self, index: int) -> Path:
        return self.folder / "up" / f"{index}.json"

    def heard(self, index: int) -> Path:
        return self.folder / "up" / f"{index}-done.json"

    @property
    def ping(self) -> Path:
        return self.folder / "down" / "ping"

    @property
    def started(self) -> Path:
        return self.folder / "term" / "started.json"

    @property
    def received(self) -> Path:
        return self.folder / "term" / "received.json"

    @property
    def states(self) -> Path:
        return self.folder / "enforce" / "states.txt"


def script(profile: Profile, folder: Path, arguments: str) -> str:
    run = shlex.join([*profile.command, "probe", str(folder)])
    return "#!/bin/bash\n" + "".join(f"{line}\n" for line in profile.resources.setup) + f"exec {run} {arguments}\n"


def release() -> str:
    try:
        return version("spit-bash")
    except PackageNotFoundError:
        return "(uninstalled)"


def user() -> str:
    try:
        import pwd
        return pwd.getpwuid(os.getuid()).pw_name
    except (ImportError, KeyError):
        return str(os.getuid())


def write_json(path: Path, content: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(content, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read_json(path: Path) -> dict[str, Any] | None:
    try:
        with open(path, encoding="utf-8") as stream:
            found = json.load(stream)
    except (OSError, ValueError):
        return None
    return found if isinstance(found, dict) else None


def read_found(path: Path) -> Found | None:
    data = read_json(path)
    return None if data is None else Found.from_json(data)


def read_heard(path: Path) -> Heard | None:
    data = read_json(path)
    return None if data is None else Heard.from_json(data)


def accounting() -> Accounting:
    task = f"{os.environ.get('SLURM_ARRAY_JOB_ID', '')}_{os.environ.get('SLURM_ARRAY_TASK_ID', '')}"
    why = ""
    for attempt in range(ACCOUNTING_TRIES):
        if attempt:
            time.sleep(ACCOUNTING_PAUSE)
        try:
            done = subprocess.run(["sacct", "-X", "-n", "-P", "-j", task, "--format=JobID,State"],
                                  capture_output=True, text=True, timeout=SACCT_TIMEOUT)
        except (OSError, subprocess.TimeoutExpired) as exc:
            return Accounting(False, f"cannot run sacct: {exc}")
        if done.returncode != 0:
            lines = done.stderr.strip().splitlines()
            return Accounting(False, lines[-1] if lines else f"sacct exited with status {done.returncode}")
        if any(line.startswith(task) for line in done.stdout.splitlines()):
            return Accounting(True)
        why = f"sacct has no row for {task}, even after {attempt * ACCOUNTING_PAUSE:.0f}s"
    return Accounting(False, why)


def terminated(paths: Paths, timeout: float) -> int:
    def received(signum: int, frame: Any) -> None:
        write_json(paths.received, {"at": time.time(), "signal": signum})
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, received)
    write_json(paths.started, {"host": socket.gethostname(), "at": time.time()})
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    return 0


def hog(megabytes: int) -> int:
    held = b"a" * (megabytes * 1024**2)
    time.sleep(5)
    return 0 if held else 1


def compare(paths: Paths, index: int, timeout: float) -> int:
    quiet = read_json(paths.ping) is None
    umask = os.umask(0)
    os.umask(umask)
    found = Found(index=index, identity=Identity.here(), umask=umask,
                  python=".".join(map(str, sys.version_info[:3])), job=os.environ.get("SLURM_JOB_ID", ""),
                  by_controller=Attempt.of(paths.by_controller), by_node=Attempt.of(paths.by_node(index)),
                  contested=Attempt.of(paths.contested), accounting=accounting(), quiet=quiet, written=time.time())
    write_json(paths.found(index), found.to_json())
    deadline = time.monotonic() + timeout
    ping = None
    while ping is None and time.monotonic() < deadline:
        ping = read_json(paths.ping)
        if ping is None:
            time.sleep(POLL_SECONDS)
    written = ping.get("written") if ping else None
    write_json(paths.heard(index), Heard(index, ping is not None, time.time(), written).to_json())
    return 0


def main(folder: Path, index: int, timeout: float, mode: Mode = Mode.CHECK) -> int:
    if mode is Mode.HOG:
        return hog(HOG_MEGABYTES)
    if mode is Mode.SLEEP:
        time.sleep(timeout)
        return 0
    paths = Paths(folder)
    return terminated(paths, timeout) if index >= TASKS else compare(paths, index, timeout)
