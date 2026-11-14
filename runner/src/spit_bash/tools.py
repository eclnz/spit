"""Resolve tool identities once, before planning any jobs."""

from __future__ import annotations

import base64
import difflib
import subprocess

from .model import DagError


def resolve(declared, used, root):
    """Opaque identities and probes' exact stdout, tagged to keep their kinds distinct."""
    definitions = {tool.name: tool for tool in declared}
    if len(definitions) != len(declared):
        raise DagError("duplicate tool identity")
    unknown = used - definitions.keys()
    if unknown:
        raise DagError(f"undefined tool identities: {', '.join(sorted(unknown))}")
    identities = {}
    for name in sorted(used):
        tool = definitions[name]
        if tool.identity is not None:
            identities[name] = "literal:" + tool.identity
            continue
        try:
            result = subprocess.run(tool.probe, cwd=root, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=30, check=False)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise DagError(f"tool probe {name} could not run: {exc}") from exc
        if result.returncode:
            raise DagError(f"tool probe {name} exited {result.returncode}: "
                           + result.stderr.decode(errors="replace")[-1000:])
        identities[name] = "stdout:" + base64.b64encode(result.stdout).decode("ascii")
    return identities


def shown(identity):
    """An identity for a change report; its stored representation stays byte-exact."""
    if identity.startswith("stdout:"):
        return base64.b64decode(identity[7:]).decode(errors="backslashreplace")
    return identity.removeprefix("literal:")


def changed_notes(dag, state):
    """Each distinct recorded-to-current identity change, shown once."""
    changes = set()
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is None:
            continue
        for name in sorted(set(record.tools) | set(job.work.tools)):
            before, after = record.tools.get(name), job.work.tools.get(name)
            if before == after or (name, before, after) in changes:
                continue
            changes.add((name, before, after))
            yield f"tool {name} identity changed:"
            yield from difflib.unified_diff(shown(before or "(not recorded)").splitlines(),
                                           shown(after or "(not used)").splitlines(),
                                           fromfile="recorded", tofile="current", lineterm="")


def accept(dag, state):
    """Accept only tool changes, retaining every file and check's evidence.

    A job with other changed work is refused before any record is written.
    File edits stay edits: accepting identities does not refresh their evidence.
    """
    updates = []
    for job in dag.jobs:
        record = state.jobs.get(job.work.key)
        if record is None or record.tools == job.work.tools:
            continue
        without = lambda parts: {name: value for name, value in (parts or {}).items() if name != "tools"}
        if record.parts is None or without(record.parts) != without(job.work.parts):
            raise DagError(f"job {job.id}: work other than tools changed; cannot accept tool identities")
        updates.append((job.work.key, record.model_copy(update={
            "fingerprint": job.work.fingerprint, "parts": job.work.parts, "tools": job.work.tools,
        })))
    for key, record in updates:
        state.record(key, record)
    return len(updates)
