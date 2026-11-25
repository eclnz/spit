from __future__ import annotations

import asyncio
import dataclasses
import hashlib
import json
import os
import secrets
import shutil
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from ..model import DagError, Layout
from . import behaviour, commands, probe
from .checks import Check, Name, Status
from .judge import Seen, judge
from .profile import Profile, Resources, sbatch_options

STALE_SECONDS = 14 * 86400.0
REPORT_VERSION = 1
PROBE_MEMORY = 512 * 1024**2
PROBE_MARGIN = 120.0
PROBE_NAME = "spit-site"
REQUIRED_COMMANDS = ("sbatch", "squeue", "scancel", "scontrol")


@dataclass(frozen=True)
class Report:
    at: float
    fingerprint: str
    host: str
    checks: tuple[Check, ...]
    kept: str = ""
    version: int = REPORT_VERSION

    @property
    def failed(self) -> tuple[Check, ...]:
        return tuple(check for check in self.checks if check.status is Status.FAIL)

    def to_json(self) -> dict[str, Any]:
        return {
            "at": self.at,
            "fingerprint": self.fingerprint,
            "host": self.host,
            "kept": self.kept,
            "version": self.version,
            "checks": [[check.name.value, check.status.value, check.detail] for check in self.checks],
        }

    @staticmethod
    def from_json(data: dict[str, Any]) -> Report | None:
        try:
            return Report(
                float(data["at"]),
                str(data["fingerprint"]),
                str(data["host"]),
                tuple(Check(Name(name), Status(status), detail) for name, status, detail in data["checks"]),
                str(data.get("kept", "")),
                int(data.get("version", 0)),
            )
        except (KeyError, TypeError, ValueError):
            return None


def fingerprint(profile: Profile) -> str:
    parts = [
        dataclasses.asdict(profile.resources),
        profile.operations,
        profile.stages,
        list(profile.command),
        profile.visibility,
    ]
    return hashlib.sha256(json.dumps(parts, sort_keys=True, default=str).encode()).hexdigest()[:16]


def folder(layout: Layout) -> Path:
    return layout.folder / "site"


def save(layout: Layout, report: Report) -> None:
    data = report.to_json()
    probe.write_json(folder(layout) / "profiles" / (report.fingerprint + ".json"), data)
    probe.write_json(folder(layout) / "site.json", data)


def load(layout: Layout, profile: Profile | None = None) -> Report | None:
    root = folder(layout)
    data = None if profile is None else probe.read_json(root / "profiles" / (fingerprint(profile) + ".json"))
    if data is None:
        data = probe.read_json(root / "site.json")
    return None if data is None else Report.from_json(data)


def gate(profile: Profile, layout: Layout, now: float | None = None) -> list[str]:
    report = load(layout, profile)
    if report is None or report.fingerprint != fingerprint(profile):
        if profile.require_site_check:
            raise DagError(
                "this profile requires a site check, and none has been made for it: run "
                "`spit-bash check-site` with the same profile first"
            )
        return ["no site check has been made for this profile; `spit-bash check-site` tests the cluster before a run"]
    if report.failed:
        listing = "; ".join(f"{check.name.value}: {check.detail}" for check in report.failed)
        raise DagError(
            f"the site check failed for this profile, so a run would not be safe: {listing}. Fix what it "
            "names, or change the profile, then run `spit-bash check-site` again"
        )
    if report.version != REPORT_VERSION:
        message = "the site report uses an older set of checks; run `spit-bash check-site` again for this profile"
        if profile.require_site_check:
            raise DagError(message)
        return [message]
    age = (time.time() if now is None else now) - report.at
    if age > STALE_SECONDS:
        return [f"the site check is {age / 86400:.0f} days old; `spit-bash check-site` tests the cluster again"]
    return []


def requests(profile: Profile) -> list[Resources]:
    found: list[Resources] = [profile.resources]
    for stage in ((), *(tuple(name.split("/")) for name in sorted(profile.stages))):
        for operation in ("", *sorted(profile.operations)):
            each = profile.resources_for_operation(operation, stage)
            if each not in found:
                found.append(each)
    return found


def script(profile: Profile, probes: Path, timeout: float) -> str:
    return probe.script(profile, probes, f'"$SLURM_ARRAY_TASK_ID" --timeout {timeout:g}')


async def _commands() -> list[Check]:
    try:
        commands.require_local_cluster()
    except DagError as exc:
        return [Check(Name.COMMANDS, Status.FAIL, str(exc))]
    missing = [name for name in REQUIRED_COMMANDS if shutil.which(name) is None]
    if missing:
        return [Check(Name.COMMANDS, Status.FAIL, f"{', '.join(missing)} not found on PATH; is Slurm installed here?")]
    found = [
        Check(
            Name.COMMANDS, Status.PASS, f"{', '.join(REQUIRED_COMMANDS[:-1])} and {REQUIRED_COMMANDS[-1]} are on PATH"
        )
    ]
    if shutil.which("sacct") is None:
        found.append(
            Check(Name.ACCOUNTING, Status.WARN, "sacct is not on PATH here, so a run cannot say why Slurm ended a task")
        )
    return found


async def _requests(profile: Profile) -> Check:
    refused = []
    asked = requests(profile)
    for index, resources in enumerate(asked):
        if index:
            await asyncio.sleep(profile.per / profile.submissions)
        where = ", ".join(
            part
            for part in (
                f"account {resources.account}" if resources.account else "",
                f"partition {resources.partition}" if resources.partition else "",
            )
            if part
        )
        try:
            code, out, err = await commands.call(
                [
                    "sbatch",
                    "--test-only",
                    "--parsable",
                    f"--job-name={PROBE_NAME}",
                    "--wrap=true",
                    *sbatch_options(resources, resources.memory, resources.time),
                ]
            )
        except commands.SlurmError as exc:
            return Check(Name.PROFILE, Status.FAIL, str(exc))
        if code != 0:
            words = (err.strip() or out.strip() or f"exited with status {code}").splitlines()[-1]
            refused.append(f"{where or 'the defaults'}: {words}")
    if refused:
        return Check(Name.PROFILE, Status.FAIL, "Slurm would not accept the profile's request: " + "; ".join(refused))
    many = "" if len(asked) == 1 else f" for each of {len(asked)} requests"
    return Check(Name.PROFILE, Status.PASS, f"Slurm accepts the profile's request, by `sbatch --test-only`{many}")


def _probe_request(profile: Profile, timeout: float) -> tuple[int, float]:
    return min(profile.resources.memory, PROBE_MEMORY), min(profile.resources.time, timeout + PROBE_MARGIN)


def plan_lines(profile: Profile, probes: Path, timeout: float, enforcement: bool = False) -> list[str]:
    small = sbatch_options(profile.resources, *_probe_request(profile, timeout))
    lines = [
        "would ask `scontrol show config` for the site's limits, and test, with `sbatch --test-only`, which "
        "queues nothing:"
    ]
    for resources in requests(profile):
        lines.append("    sbatch " + " ".join(sbatch_options(resources, resources.memory, resources.time)))
    lines += [
        "would queue a held array, which uses no node, list it with `squeue`, and cancel it with `scancel "
        f"--name={behaviour.QUEUE_NAME}`:",
        "    sbatch --hold --array=0-1 " + " ".join(small),
        f"would submit one array of {probe.TASKS + 1} probe tasks, in {probes}, wait up to {timeout:g}s for "
        "files from them, looking at the folder and not at the queue, and cancel the last, which waits for "
        "SIGTERM:",
        "    " + " ".join(["sbatch", f"--array=0-{probe.TASKS}", *small]),
        "the probe script:",
    ]
    lines += ["    " + line for line in script(profile, probes, timeout).splitlines()]
    lines.append("then one sacct call for the first task, and scancel only if a task has not ended")
    if enforcement:
        lines.append(
            "and, for --enforcement, a job that uses more memory than it asks for, one that outlasts a "
            "one-minute limit, and a third that depends on both and writes how Slurm called them"
        )
    return lines


async def check(
    profile: Profile,
    layout: Layout,
    timeout: float = 300.0,
    poll: float = probe.POLL_SECONDS,
    enforcement: bool = False,
    say: Callable[[str], None] = lambda line: None,
) -> Report:
    checks: list[Check] = []
    probes = folder(layout) / f"probe-{time.strftime('%Y%m%d-%H%M%S')}-{os.getpid()}-{secrets.token_hex(8)}"
    paths = probe.Paths(probes)
    name = behaviour.probe_name(PROBE_NAME, probes)
    memory, time_limit = _probe_request(profile, timeout)
    array: str | None = None
    submission_pending = False
    seen = [Seen() for _ in range(probe.TASKS)]

    async def run() -> None:
        nonlocal array, submission_pending
        checks.extend(await _commands())
        if any(check.status is Status.FAIL for check in checks):
            return
        checks.append(await _requests(profile))
        if checks[-1].status is Status.FAIL:
            return
        limited, kill_wait = behaviour.limits(profile, await commands.check_site_values())
        checks.append(limited)
        paths.make()
        probe.Attempt.of(paths.by_controller)
        for index in range(probe.TASKS):
            for path in (paths.found(index), paths.heard(index), paths.by_node(index)):
                probe.read_json(path)
        text = probes / "probe.sh"
        text.write_text(script(profile, probes, timeout), encoding="utf-8")
        submission_pending = True
        try:
            array = await commands.submit(
                text,
                [
                    f"--array=0-{probe.TASKS}",
                    f"--job-name={name}",
                    f"--output={probes / '%a.out'}",
                    *sbatch_options(profile.resources, memory, time_limit),
                ],
            )
        except commands.Unsure:
            raise
        except commands.SlurmError:
            submission_pending = False
            raise
        submission_pending = False
        say(f"submitted probe array {array}; waiting for its tasks to write their files")
        checks.append(await behaviour.queue(profile, probes, memory, time_limit))
        if checks[-1].status is Status.FAIL:
            return
        signal = await _watch(paths, seen, array, timeout, poll, profile.visibility + (kill_wait or 30.0))
        accounted = await commands.accounting([f"{array}_0"])
        login = accounted is not None and f"{array}_0" in accounted
        checks.extend(judge(profile, seen, probe.Identity.here(), login))
        checks.append(behaviour.stop_signal(signal, kill_wait))
        if enforcement and not any(check.status is Status.FAIL for check in checks):
            say("ending a job for its memory and one for its time on purpose; this takes over a minute")
            checks.extend(await behaviour.enforcement(profile, paths, time_limit, timeout, poll))

    interrupted = False
    try:
        await asyncio.wait_for(run(), timeout)
    except asyncio.TimeoutError:
        words = "no probe task reported; " if array and all(each.found is None for each in seen) else ""
        checks.append(Check(Name.PROBE, Status.FAIL, f"{words}site check exceeded its {timeout:g}s deadline"))
        interrupted = True
    except asyncio.CancelledError:
        checks.append(Check(Name.PROBE, Status.FAIL, "site check interrupted"))
        interrupted = True
    except (OSError, commands.SlurmError) as exc:
        checks.append(Check(Name.PROBE, Status.FAIL, f"could not complete the probe: {exc}"))
        interrupted = True
    failed = any(check.status is Status.FAIL for check in checks)
    if submission_pending:
        commands.cancel_name(name)
        checks.append(
            Check(
                Name.PROBE,
                Status.FAIL,
                f"submission under {name} unconfirmed; cancellation requested by its unique probe name",
            )
        )
        failed = True
    elif array and (interrupted or failed or any(each.heard is None for each in seen)):
        cancel_failed = commands.cancel([array])
        checks.append(
            Check(
                Name.PROBE,
                Status.FAIL if cancel_failed else Status.WARN,
                f"{'could not request' if cancel_failed else 'requested'} cancellation of {array}; "
                "terminal status unconfirmed",
            )
        )
        failed |= bool(cancel_failed)
    if not failed:
        shutil.rmtree(probes, ignore_errors=True)
    return _finish(profile, layout, checks, probes if failed and probes.exists() else None)


@dataclass
class Stop:
    cancelled_at: float | None = None
    received: bool = False

    def outcome(self, grace: float) -> behaviour.Signal:
        if self.received:
            return behaviour.Signal.RECEIVED
        if self.cancelled_at is not None and time.monotonic() - self.cancelled_at > grace:
            return behaviour.Signal.MISSING
        return behaviour.Signal.UNTRIED


def _read_tasks(paths: probe.Paths, seen: list[Seen]) -> None:
    for index, each in enumerate(seen):
        if each.found is None:
            each.found = probe.read_found(paths.found(index))
            if each.found is not None:
                each.found_at = time.time()
                each.blocked = probe.Attempt.of(paths.by_node(index))
        elif each.heard is None:
            each.heard = probe.read_heard(paths.heard(index))


def _watch_stop(paths: probe.Paths, array: str, stop: Stop) -> None:
    if stop.cancelled_at is None:
        if probe.read_json(paths.started) is not None:
            commands.cancel([f"{array}_{probe.TASKS}"])
            stop.cancelled_at = time.monotonic()
    elif not stop.received:
        stop.received = probe.read_json(paths.received) is not None


async def _watch(
    paths: probe.Paths, seen: list[Seen], array: str, timeout: float, poll: float, grace: float
) -> behaviour.Signal:
    deadline = time.monotonic() + timeout
    stop = Stop()
    pinged = False
    while time.monotonic() < deadline:
        _read_tasks(paths, seen)
        if not pinged and any(each.found is not None for each in seen):
            probe.write_json(paths.ping, {"written": time.time()})
            pinged = True
        _watch_stop(paths, array, stop)
        if all(each.heard is not None for each in seen) and stop.outcome(grace) is not behaviour.Signal.UNTRIED:
            break
        await asyncio.sleep(poll)
    return stop.outcome(grace)


def _finish(profile: Profile, layout: Layout, checks: list[Check], kept: Path | None = None) -> Report:
    report = Report(
        time.time(), fingerprint(profile), probe.Identity.here().host, tuple(checks), "" if kept is None else str(kept)
    )
    save(layout, report)
    return report


def lines(report: Report) -> list[str]:
    width = max((len(check.name.value) for check in report.checks), default=0)
    found = [f"{check.status.value:<4}  {check.name.value:<{width}}  {check.detail}" for check in report.checks]
    counts = {status: sum(check.status is status for check in report.checks) for status in Status}
    found.append(
        f"site check: {counts[Status.PASS]} passed, {counts[Status.WARN]} warning"
        f"{'' if counts[Status.WARN] == 1 else 's'}, {counts[Status.FAIL]} failed"
    )
    if report.kept:
        found.append(f"the probe's folder, with its tasks' logs, is kept at {report.kept}")
    return found
