from __future__ import annotations

import math
from dataclasses import dataclass

from . import probe
from .checks import Check, Name, Status
from .probe import Claim
from .profile import Profile

SKEW_SECONDS = 1.0


@dataclass
class Seen:
    found: probe.Found | None = None
    found_at: float | None = None
    heard: probe.Heard | None = None
    blocked: probe.Attempt | None = None


def judge(profile: Profile, seen: list[Seen], here: probe.Identity, login_accounting: bool | None) -> list[Check]:
    reported = [each for each in seen if each.found is not None]
    checks = [_tasks(seen, reported)]
    if not reported:
        return checks
    checks += [_tools(reported, here), _exclusive(reported), *_visibility(profile, reported),
               _accounting(reported, login_accounting)]
    return checks


def _found(each: Seen) -> probe.Found:
    assert each.found is not None, "a task is judged only once it has reported"
    return each.found


def _said(attempt: probe.Attempt | None) -> str:
    if attempt is None:
        return "`not tried`"
    return f"`{attempt.outcome.value}: {attempt.why}`" if attempt.why else f"`{attempt.outcome.value}`"


def _tasks(seen: list[Seen], reported: list[Seen]) -> Check:
    if not reported:
        return Check(Name.PROBE, Status.FAIL, "no probe task reported: they did not start, or could not write to the "
                                              "runner's folder, whose logs are in the probe folder")
    hosts = sorted({_found(each).identity.host for each in reported})
    if len(reported) < len(seen):
        return Check(Name.PROBE, Status.WARN, f"only {len(reported)} of {len(seen)} probe tasks reported, from "
                                              f"{', '.join(hosts)}; the rest did not start in time")
    return Check(Name.PROBE, Status.PASS, f"{len(reported)} probe tasks ran, on {', '.join(hosts)}")


def _tools(reported: list[Seen], here: probe.Identity) -> Check:
    problems = []
    for each in reported:
        node = _found(each).identity
        if (node.user, node.uid) != (here.user, here.uid):
            problems.append(f"{node.host} runs jobs as {node.user} ({node.uid}), not {here.user} ({here.uid})")
        if node.spit_bash != here.spit_bash:
            problems.append(f"{node.host} has spit-bash {node.spit_bash}, not {here.spit_bash}")
        if node.dag != here.dag:
            problems.append(f"{node.host} reads DAG version {node.dag}, not {here.dag}")
    if problems:
        return Check(Name.TOOLS, Status.FAIL, "; ".join(problems))
    return Check(Name.TOOLS, Status.PASS, f"every probe node started spit-bash {here.spit_bash} as {here.user}, "
                                          "through the profile's setup lines and command")


def _exclusive(reported: list[Seen]) -> Check:
    problems = []
    for each in reported:
        found = _found(each)
        host = found.identity.host
        if found.by_controller.outcome is not Claim.EXISTS:
            problems.append(f"{host} made a file the controller had already made ({_said(found.by_controller)})")
        if found.by_node.outcome is not Claim.CREATED:
            problems.append(f"{host} could not make its own file ({_said(found.by_node)})")
        if each.blocked is None or each.blocked.outcome is not Claim.EXISTS:
            problems.append(f"the controller made a file {host} had already made ({_said(each.blocked)})")
    winners = [each for each in reported if _found(each).contested.outcome is Claim.CREATED]
    if len(winners) > 1:
        problems.append(f"{len(winners)} nodes both made the file they raced for")
    if problems:
        return Check(Name.EXCLUSIVE, Status.FAIL, "claims would not keep one task per job: " + "; ".join(problems))
    hosts = sorted({_found(each).identity.host for each in reported})
    detail = "an exclusive create is decided by the file server, between the controller and each node"
    if len(hosts) > 1:
        return Check(Name.EXCLUSIVE, Status.PASS, detail + ", and between nodes")
    return Check(Name.EXCLUSIVE, Status.PASS,
                 f"{detail}; the probe's tasks all ran on {hosts[0]}, so two nodes were not tried against each other")


def _lags(reported: list[Seen]) -> tuple[list[float], list[float]]:
    upstream: list[float] = []
    downstream: list[float] = []
    for each in reported:
        found = _found(each)
        if each.found_at is not None:
            upstream.append(each.found_at - found.written)
        heard = each.heard
        if heard is not None and heard.heard and found.quiet and heard.written is not None:
            downstream.append(heard.seen - heard.written)
    return upstream, downstream


def _visibility(profile: Profile, reported: list[Seen]) -> list[Check]:
    upstream, downstream = _lags(reported)
    if not upstream or not downstream or any(each.heard is None or not each.heard.heard for each in reported):
        return [Check(Name.VISIBILITY, Status.FAIL, "could not verify visibility in both directions; "
                                                    "a controller-to-node ping or a node observation was missing")]
    lags = upstream + downstream
    worst = max(max(lags), 0.0)
    skew = -min(lags)
    bound = profile.visibility
    advice = f"; set visibility to at least {math.ceil(worst * 2 + 1)}s"
    if worst >= bound:
        found = Check(Name.VISIBILITY, Status.FAIL,
                      f"a file took {worst:.1f}s to show on another machine, and the profile's visibility is "
                      f"{bound:.0f}s{advice}")
    elif worst > bound / 2:
        found = Check(Name.VISIBILITY, Status.WARN,
                      f"a file took {worst:.1f}s to show on another machine, over half of the profile's "
                      f"visibility, {bound:.0f}s; caches can keep an entry longer than {len(lags)} samples "
                      f"show{advice}")
    else:
        found = Check(Name.VISIBILITY, Status.PASS,
                      f"files showed on another machine within {worst:.1f}s, under the profile's visibility, "
                      f"{bound:.0f}s ({len(lags)} samples)")
    checks = [found]
    if skew > SKEW_SECONDS:
        checks.append(Check(Name.CLOCKS, Status.WARN,
                            f"a file was seen {skew:.0f}s before the machine that wrote it says it was written, so "
                            "the clocks differ, and a time one node records may look wrong on another"))
    return checks


def _accounting(reported: list[Seen], login: bool | None) -> Check:
    failed = [f"{_found(each).identity.host}: {_found(each).accounting.why}" for each in reported
              if not _found(each).accounting.ok]
    if not login:
        failed.insert(0, "the controller's machine: sacct failed or does not know the probe's task")
    if failed:
        return Check(Name.ACCOUNTING, Status.WARN,
                     "sacct does not answer everywhere, so a run cannot say why Slurm ended a task, nor retry one it "
                     "killed for memory or time: " + "; ".join(failed))
    return Check(Name.ACCOUNTING, Status.PASS, "sacct answers on the controller's machine and on every probe node")
