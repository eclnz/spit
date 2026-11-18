"""`spit-bash probe`: what a node of the site check tells the controller about itself and the shared folder.

The controller (`site.check`) makes a folder on the shared filesystem and submits tasks that run this. Each of the
first `TASKS` tests, from its node, what a run relies on, and writes what it found where the controller reads it; the
one task past them waits to be cancelled. `Paths` names every file the two sides leave each other, and what each is.

The two files each side waits for are looked at before they exist, as a run looks at a result before it is written, so
a cache that remembers their absence delays them, and the delay is what the check measures. A run relies on SIGTERM
reaching a cancelled or timed-out task, so that it can put back the outputs it was replacing.
"""

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

# How often a node looks for the controller's ping; the controller looks for a node's files as often.
POLL_SECONDS = 1.0
# How long `sacct` may take on a node.
SACCT_TIMEOUT = 60.0
# How many tasks compare the controller with a node. One more waits for SIGTERM.
TASKS = 2
# What the memory check's job holds, which is more than the memory it asks for.
HOG_MEGABYTES = 400
# How many times, and how far apart, a node asks `sacct` for its own task before it calls the row missing.
ACCOUNTING_TRIES = 6
ACCOUNTING_PAUSE = 2.0


class Mode(str, Enum):
    """What a probe job does."""

    # One of the tasks that compare a node with the controller, or the one that waits for SIGTERM.
    CHECK = "check"
    # Use more memory than the job asked for, for the cgroup to end it.
    HOG = "hog"
    # Outlast the job's time limit, for Slurm to end it.
    SLEEP = "sleep"


class Claim(str, Enum):
    """What an exclusive create came to, as a claim is taken."""

    CREATED = "created"
    # The file was there, which is what a file server that decides the create says to the later of two.
    EXISTS = "exists"
    # It failed some other way, as `Attempt.why` says.
    FAILED = "failed"


@dataclass(frozen=True)
class Attempt:
    """One exclusive create, and why it failed if it did."""

    outcome: Claim
    why: str = ""

    @staticmethod
    def of(path: Path) -> Attempt:
        """Try to create `path` as a claim is taken, with an exclusive create."""
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
    """Whether `sacct` answers on a machine, and knows the probe's task."""

    ok: bool
    why: str = ""


@dataclass(frozen=True)
class Identity:
    """What a machine says it is, for the controller to compare with each node."""

    user: str
    uid: int
    spit_bash: str
    # The DAG format version this spit-bash reads.
    dag: int
    host: str

    @staticmethod
    def here() -> Identity:
        return Identity(user(), os.getuid(), release(), DAG_VERSION, socket.gethostname())


@dataclass(frozen=True)
class Found:
    """What one of the first `TASKS` tasks found, which it writes when it has found it all."""

    index: int
    identity: Identity
    umask: int
    python: str
    # Slurm's job number for the task.
    job: str
    # The task's exclusive creates: of a file the controller made first, of its own, and of the one all tasks race for.
    by_controller: Attempt
    by_node: Attempt
    contested: Attempt
    accounting: Accounting
    # Whether the task's first look for the ping found nothing, so that its cache held the ping's absence and the
    # time the ping then took to show is the lag.
    quiet: bool
    # When the task wrote this, by its clock.
    written: float

    def to_json(self) -> dict[str, Any]:
        return dataclasses.asdict(self)

    @staticmethod
    def from_json(data: dict[str, Any]) -> Found | None:
        """What `to_json` wrote, or None where it is not that."""
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
    """When a task saw the controller's ping, which it writes last."""

    index: int
    # Whether it saw the ping before its own time ran out.
    heard: bool
    # When it saw the ping, by its clock, and when the controller says it wrote it.
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
    """Every file the controller and the nodes leave each other in a probe's folder, and what each is for."""

    folder: Path

    def make(self) -> None:
        """Make the folders the files are in."""
        for each in ("exclusive", "up", "down", "term", "enforce"):
            (self.folder / each).mkdir(parents=True, exist_ok=True)

    @property
    def by_controller(self) -> Path:
        """Made by the controller before the tasks start; a node's exclusive create of it must fail."""
        return self.folder / "exclusive" / "by-controller"

    def by_node(self, index: int) -> Path:
        """Made by task `index`; the controller's exclusive create of it must then fail."""
        return self.folder / "exclusive" / f"by-node-{index}"

    @property
    def contested(self) -> Path:
        """Every task tries to make it; one at most may."""
        return self.folder / "exclusive" / "contested"

    def found(self, index: int) -> Path:
        """What task `index` is and found, as `Found`."""
        return self.folder / "up" / f"{index}.json"

    def heard(self, index: int) -> Path:
        """When task `index` saw the ping, as `Heard`."""
        return self.folder / "up" / f"{index}-done.json"

    @property
    def ping(self) -> Path:
        """Written by the controller once it sees a `found` file; each task waits for it."""
        return self.folder / "down" / "ping"

    @property
    def started(self) -> Path:
        """Written by the task that waits to be cancelled, when it starts."""
        return self.folder / "term" / "started.json"

    @property
    def received(self) -> Path:
        """Written by that task when SIGTERM reaches it."""
        return self.folder / "term" / "received.json"

    @property
    def states(self) -> Path:
        """Written by the job that depends on the enforcement jobs: how Slurm called each."""
        return self.folder / "enforce" / "states.txt"


def script(profile: Profile, folder: Path, arguments: str) -> str:
    """The script of a job that runs `spit-bash probe` as a run starts spit-bash on a node: the profile's setup lines,
    then its command. `arguments` follow the folder, as written for the shell."""
    run = shlex.join([*profile.command, "probe", str(folder)])
    return "#!/bin/bash\n" + "".join(f"{line}\n" for line in profile.resources.setup) + f"exec {run} {arguments}\n"


def release() -> str:
    """This runner's version, as `--version` shows it."""
    try:
        return version("spit-bash")
    except PackageNotFoundError:
        return "(uninstalled)"


def user() -> str:
    """The user running this process, by name where the system has one, else by number."""
    try:
        import pwd
        return pwd.getpwuid(os.getuid()).pw_name
    except (ImportError, KeyError):
        return str(os.getuid())


def write_json(path: Path, content: dict[str, Any]) -> None:
    """Write `content` so a reader sees all of it or none: to a temporary name, synced, then renamed."""
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
    """What `write_json` wrote at `path`, or None while it is not there. Opening the file makes an NFS client ask the
    server about it, so a file is read, not only looked for."""
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
    """Whether `sacct` answers on this node, and knows this task.

    A task asks as it starts, and accounting may not have its row yet, so a call that answers without one is made
    again, a few times, before the row is called missing.
    """
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
    """The task that waits for SIGTERM: say when it starts, and when the signal arrives."""
    def received(signum: int, frame: Any) -> None:
        write_json(paths.received, {"at": time.time(), "signal": signum})

    signal.signal(signal.SIGTERM, received)
    write_json(paths.started, {"host": socket.gethostname(), "at": time.time()})
    # Slurm ends the task, with SIGKILL if need be, after `scancel`; this is only the longest it waits.
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    return 0


def hog(megabytes: int) -> int:
    """Use more memory than the job asked for, touching every page, for the cgroup to end it."""
    held = b"a" * (megabytes * 1024**2)
    time.sleep(5)
    return 0 if held else 1


def compare(paths: Paths, index: int, timeout: float) -> int:
    """Run task `index`, one of those that compare this node with the controller."""
    # The first look at the ping, before the controller writes it, as a run looks for a result before it exists.
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
    """Run task `index` of the check in `folder`; the status of the process.

    The probe's tasks are those below `TASKS`, which compare this node with the controller, and the one past them,
    which waits for SIGTERM. `Mode.HOG` and `Mode.SLEEP` are the jobs that check Slurm enforces memory and time.
    """
    if mode is Mode.HOG:
        return hog(HOG_MEGABYTES)
    if mode is Mode.SLEEP:
        time.sleep(timeout)
        return 0
    paths = Paths(folder)
    return terminated(paths, timeout) if index >= TASKS else compare(paths, index, timeout)
