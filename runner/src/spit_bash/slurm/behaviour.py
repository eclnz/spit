from __future__ import annotations

import asyncio
import hashlib
import os
import shlex
import time
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from . import commands, probe
from .checks import Check, Name, Status
from .profile import Profile, sbatch_options, status_intervals
from .reconcile import ENDED

QUEUE_NAME = "spit-site-queue"
MIN_KILL_WAIT = 10.0
CANCEL_LOOKS = 5
SMALL_MEMORY = 256 * 1024**2
MEMORY_ASKED = 64 * 1024**2
TIME_ASKED = 60.0
SLEEP_SECONDS = 150.0
RECORDER_LIMIT = 600.0
RECORDER_TRIES = 10
RECORDER_PAUSE = 3


class Signal(str, Enum):
    RECEIVED = "received"
    MISSING = "missing"
    UNTRIED = "untried"


def _seconds(values: dict[str, str], key: str) -> float | None:
    text = values.get(key, "")
    return float(text) if text.isdigit() else None


def limits(profile: Profile, values: dict[str, str] | None) -> tuple[Check, float | None]:
    defaults = commands.Config()
    if values is None:
        return Check(
            Name.LIMITS,
            Status.WARN,
            f"scontrol does not answer, so a run assumes MinJobAge {defaults.min_job_age:.0f}s and "
            f"MaxArraySize {defaults.max_array}, which may not be the site's",
        ), None
    age = _seconds(values, "MinJobAge")
    array = _seconds(values, "MaxArraySize")
    kill = _seconds(values, "KillWait")
    age = defaults.min_job_age if age is None else age
    facts = [f"MinJobAge {age:.0f}s"]
    if array is not None:
        facts.append(f"MaxArraySize {array:.0f}")
    if kill is not None:
        facts.append(f"KillWait {kill:.0f}s")
    interval, ceiling = status_intervals(profile, age)
    if interval < profile.status_interval:
        return Check(
            Name.LIMITS,
            Status.WARN,
            f"{', '.join(facts)}: a run reads the queue every {interval:g}s, not the profile's "
            f"{profile.status_interval:g}s, due to the effective polling ceiling",
        ), kill
    return Check(
        Name.LIMITS,
        Status.PASS,
        f"{', '.join(facts)}; a run reads the queue every {interval:g}s while "
        f"its tasks change, and at most every {ceiling:g}s while they do not",
    ), kill


async def queue(profile: Profile, probes: Path, memory: int, time_limit: float) -> Check:
    script = probes / "queue.sh"
    script.write_text("#!/bin/bash\nexit 0\n", encoding="utf-8")
    throttle = f"%{profile.array_limit}" if profile.array_limit else ""
    name = probe_name(QUEUE_NAME, probes)
    arguments = [
        "--hold",
        f"--array=0-1{throttle}",
        f"--job-name={name}",
        "--comment=spit:site",
        f"--output={probes / 'queue-%a.out'}",
        *sbatch_options(profile.resources, memory, time_limit),
    ]
    cancellation_requested = False
    try:
        array = await commands.submit(script, arguments)
        problems = _listing_problems(await commands.snapshot(name), array, script)
        if not problems:
            if commands.cancel([f"{array}_0"]):
                problems.append(f"could not cancel only probe element {array}_0")
            else:
                listed = await commands.snapshot(name)
                if listed is None or not any(row.task == f"{array}_1" and row.state == "PENDING" for row in listed):
                    problems.append(
                        f"could not verify that sibling {array}_1 stayed pending after cancelling {array}_0"
                    )
        cancellation_requested = True
        cancelled = await asyncio.to_thread(commands.cancel_name, name)
        after = await _after_cancel(name)
    except asyncio.CancelledError:
        if not cancellation_requested:
            commands.cancel_name(name)
        raise
    except commands.Unsure as exc:
        commands.cancel_name(name)
        return Check(Name.QUEUE, Status.FAIL, f"held array acceptance unconfirmed for {name}: {exc}")
    except commands.SlurmError as exc:
        return Check(Name.QUEUE, Status.FAIL, f"could not queue a held array: {exc}")
    if not cancelled:
        problems.append("`scancel --name` failed")
    if after is None:
        problems.append(f"could not read the queue to check cancellation of {array}")
    elif after[0]:
        remaining = after[0]
        problems.append(
            f"`scancel --name` left {', '.join(remaining)} in the queue, so a stopped run would leave tasks"
        )
    if problems:
        return Check(Name.QUEUE, Status.FAIL, "; ".join(problems))
    return Check(
        Name.QUEUE,
        Status.PASS,
        "`squeue` listed each task with its index and script path. "
        "Cancelling one element left its sibling pending. "
        "The held array was no longer listed as live after name cancellation.",
    )


def probe_name(prefix: str, probes: Path) -> str:
    return f"{prefix}-{hashlib.sha256(os.fsencode(probes)).hexdigest()[:16]}"


def _listing_problems(listed: list[commands.Listed] | None, array: str, script: Path) -> list[str]:
    if listed is None:
        return ["squeue did not list the array's tasks"]
    found = {each.task: each for each in listed}
    problems = []
    for index in range(2):
        task = f"{array}_{index}"
        each = found.get(task)
        if each is None:
            problems.append(f"squeue does not list {task}, as `-r` should list each task of an array")
        elif each.index != index:
            problems.append(f"squeue gives {task} the index {each.index}, not {index}")
        elif each.command != str(script):
            problems.append(f"squeue shows {task}'s command as `{each.command}`, not the script's path {script}")
    return problems


async def _after_cancel(name: str) -> tuple[list[str], bool] | None:
    remaining: list[str] = []
    listed_as_ended = False
    for look in range(CANCEL_LOOKS):
        if look:
            await asyncio.sleep(1.0)
        after = await commands.snapshot(name)
        if after is None:
            return None
        remaining = [f"{each.task} {each.state}" for each in after if each.state not in ENDED]
        listed_as_ended = any(each.state in ENDED for each in after)
        if not remaining:
            break
    return remaining, listed_as_ended


def stop_signal(signal: Signal, kill_wait: float | None) -> Check:
    if signal is Signal.UNTRIED:
        return Check(
            Name.STOP_SIGNAL,
            Status.WARN,
            "the task to be cancelled did not start, or was cancelled too late to tell, so it is not known "
            "whether Slurm sends SIGTERM to a cancelled task",
        )
    if signal is Signal.MISSING:
        return Check(
            Name.STOP_SIGNAL,
            Status.FAIL,
            "a task that was cancelled never received SIGTERM, so a job stopped by `scancel` or its time "
            "limit could not put back the outputs it was replacing",
        )
    if kill_wait is not None and kill_wait < MIN_KILL_WAIT:
        return Check(
            Name.STOP_SIGNAL,
            Status.WARN,
            f"SIGTERM reaches a cancelled task, but Slurm kills it {kill_wait:.0f}s later (KillWait), which "
            "may be too little to put back large outputs",
        )
    after = "" if kill_wait is None else f", and Slurm waits {kill_wait:.0f}s (KillWait) before it kills"
    return Check(Name.STOP_SIGNAL, Status.PASS, f"SIGTERM reached a cancelled task{after}")


@dataclass(frozen=True)
class Enforced:
    check: Name
    name: str
    mode: probe.Mode
    memory: int
    time_limit: float
    expected: str
    what: str


ENFORCED = (
    Enforced(
        Name.MEMORY,
        "memory",
        probe.Mode.HOG,
        MEMORY_ASKED,
        RECORDER_LIMIT,
        "OUT_OF_MEMORY",
        f"a job that used {probe.HOG_MEGABYTES}MB of the {MEMORY_ASKED // 1024**2}MB it asked for",
    ),
    Enforced(
        Name.TIME_LIMIT,
        "time",
        probe.Mode.SLEEP,
        SMALL_MEMORY,
        TIME_ASKED,
        "TIMEOUT",
        "a job that outlasted its one-minute limit",
    ),
)

RECORDER = """#!/bin/bash
states={states}
for attempt in $(seq {tries}); do
    sacct -X -n -P -j {jobs} --format=JobID,State > "$states.tmp" 2>/dev/null
    [ "$(grep -c -E '\\|({ended})' "$states.tmp")" -ge {count} ] && break
    sleep {pause}
done
mv "$states.tmp" "$states"
"""


async def enforcement(
    profile: Profile, paths: probe.Paths, time_limit: float, timeout: float, poll: float
) -> list[Check]:
    jobs: list[str] = []
    name = probe_name("spit-site-enforcement", paths.folder)
    try:
        for each in ENFORCED:
            jobs.append(await _submit_enforced(profile, paths.folder, each, min(each.time_limit, time_limit)))
        recorder = await _submit_recorder(profile, paths, jobs)
        text = await _read(paths.states, timeout, poll)
    except asyncio.CancelledError:
        commands.cancel_name(name)
        raise
    except commands.Unsure as exc:
        commands.cancel_name(name)
        return [Check(Name.ENFORCEMENT, Status.FAIL, f"enforcement acceptance unconfirmed for {name}: {exc}")]
    except (OSError, commands.SlurmError) as exc:
        commands.cancel(jobs)
        return [Check(Name.ENFORCEMENT, Status.WARN, f"could not start the jobs that test it: {exc}")]
    if text is None:
        outstanding = [*jobs, recorder]
        commands.cancel(outstanding)
        return [
            Check(
                Name.ENFORCEMENT,
                Status.FAIL,
                f"enforcement did not report in time; cancellation requested "
                f"for {', '.join(outstanding)}, terminal state unconfirmed",
            )
        ]
    called = _states(text)
    return [_enforced(each, called.get(job, "")) for each, job in zip(ENFORCED, jobs)]


async def _submit_enforced(profile: Profile, probes: Path, each: Enforced, time_limit: float) -> str:
    script = probes / f"{each.name}.sh"
    script.write_text(
        probe.script(profile, probes, f"0 --mode {each.mode.value} --timeout {SLEEP_SECONDS:g}"), encoding="utf-8"
    )
    return await commands.submit(
        script,
        [
            f"--job-name={probe_name('spit-site-enforcement', probes)}",
            f"--output={probes / (each.name + '.out')}",
            *sbatch_options(profile.resources, each.memory, time_limit),
        ],
    )


async def _submit_recorder(profile: Profile, paths: probe.Paths, jobs: list[str]) -> str:
    probes = paths.folder
    script = probes / "recorder.sh"
    script.write_text(
        RECORDER.format(
            states=shlex.quote(str(paths.states)),
            jobs=",".join(jobs),
            count=len(jobs),
            tries=RECORDER_TRIES,
            pause=RECORDER_PAUSE,
            ended="|".join(sorted(ENDED)),
        ),
        encoding="utf-8",
    )
    return await commands.submit(
        script,
        [
            f"--dependency=afterany:{':'.join(jobs)}",
            f"--job-name={probe_name('spit-site-enforcement', probes)}",
            f"--output={probes / 'recorder.out'}",
            *sbatch_options(profile.resources, SMALL_MEMORY, RECORDER_LIMIT),
        ],
    )


async def _read(path: Path, timeout: float, poll: float) -> str | None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            return path.read_text(encoding="utf-8")
        except OSError:
            await asyncio.sleep(poll)
    return None


def _states(text: str) -> dict[str, str]:
    found = {}
    for line in text.splitlines():
        job, _, state = line.partition("|")
        found[job.partition("_")[0]] = state.split()[0] if state.split() else ""
    return found


def _enforced(each: Enforced, state: str) -> Check:
    if state == each.expected:
        return Check(each.check, Status.PASS, f"{each.what} ended {each.expected}, the state a run retries on")
    return Check(
        each.check,
        Status.WARN,
        f"{each.what} ended {state or 'with no state'}, not {each.expected}, so a run cannot tell that a job "
        "needs more, and `retries` will not apply",
    )
