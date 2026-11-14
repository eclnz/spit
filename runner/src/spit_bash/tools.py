"""What each tool is on this machine: a declared text, or a probe's output, found once before any job runs."""

from __future__ import annotations

import difflib
import hashlib
import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

from . import schema
from .model import DagError

# The most lines of a probe's changed output shown, so a noisy probe is told apart from an upgrade.
SHOWN = 12


@dataclass(frozen=True)
class Identity:
    """One tool's identity: the text that identifies it, and its digest, which a job's work holds."""

    name: str
    # The declared text, or what the probe printed, decoded for showing.
    text: str
    digest: str


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def identify(tools: list[schema.Tool], cwd: Path) -> dict[str, Identity]:
    """Each tool's identity, running each probe once from `cwd`, with no shell.

    A probe that cannot start or exits with a status other than 0 stops the run before any job.
    """
    found: dict[str, Identity] = {}
    for tool in tools:
        if tool.name in found:
            raise DagError(f"tool {tool.name} is listed twice in `tools`")
        if tool.probe is None:
            assert tool.identity is not None, "the schema gives a tool an identity or a probe"
            found[tool.name] = Identity(tool.name, tool.identity, _digest(tool.identity.encode()))
            continue
        try:
            result = subprocess.run(tool.probe, cwd=cwd, stdin=subprocess.DEVNULL,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=os.environ)
        except OSError as exc:
            raise DagError(f"cannot run the probe of tool {tool.name}, {' '.join(tool.probe)}: {exc.strerror}") from exc
        if result.returncode != 0:
            said = result.stderr.decode(errors="replace").strip().splitlines()[-1:]
            reason = f": {said[0]}" if said else ""
            raise DagError(f"the probe of tool {tool.name}, {' '.join(tool.probe)}, "
                           f"exited with status {result.returncode}{reason}")
        found[tool.name] = Identity(tool.name, result.stdout.decode(errors="replace"), _digest(result.stdout))
    return found


def saved(path: Path) -> dict[str, str]:
    """The tools' texts the last run or `accept-tools` recorded, by name; empty when none were."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return {name: text for name, text in data.items() if isinstance(text, str)} if isinstance(data, dict) else {}


def save(path: Path, identities: dict[str, Identity]) -> None:
    """Record the tools' texts beside the state, for the next plan to say what changed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps({name: identity.text for name, identity in sorted(identities.items())}, indent=1)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(text + "\n", encoding="utf-8")
    temporary.replace(path)


def change_lines(before: dict[str, str], now: dict[str, Identity]) -> list[str]:
    """For each tool whose text differs from the one recorded, a line naming it and its changed lines."""
    lines: list[str] = []
    for name, identity in now.items():
        old = before.get(name)
        if old is None or old == identity.text:
            continue
        lines.append(f"tool {name} changed since it was last recorded:")
        diff = [line for line in difflib.unified_diff(old.splitlines(), identity.text.splitlines(), lineterm="", n=0)
                if line.startswith(("-", "+")) and not line.startswith(("---", "+++"))]
        lines.extend(f"  {line}" for line in diff[:SHOWN])
        if len(diff) > SHOWN:
            lines.append(f"  ... {len(diff) - SHOWN} more changed lines")
    return lines
