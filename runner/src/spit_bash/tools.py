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

SHOWN = 12


@dataclass(frozen=True)
class Identity:
    name: str
    text: str
    digest: str


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def identify(tools: list[schema.Tool], cwd: Path) -> dict[str, Identity]:
    found: dict[str, Identity] = {}
    for tool in tools:
        if tool.name in found:
            raise DagError(f"tool {tool.name} is listed twice in `tools`")
        if tool.probe is None:
            assert tool.identity is not None, "the schema gives a tool an identity or a probe"
            found[tool.name] = Identity(tool.name, tool.identity, _digest(tool.identity.encode()))
            continue
        try:
            result = subprocess.run(
                tool.probe,
                cwd=cwd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=os.environ,
            )
        except OSError as exc:
            raise DagError(f"cannot run the probe of tool {tool.name}, {' '.join(tool.probe)}: {exc.strerror}") from exc
        if result.returncode != 0:
            said = result.stderr.decode(errors="replace").strip().splitlines()[-1:]
            reason = f": {said[0]}" if said else ""
            raise DagError(
                f"the probe of tool {tool.name}, {' '.join(tool.probe)}, exited with status {result.returncode}{reason}"
            )
        found[tool.name] = Identity(tool.name, result.stdout.decode(errors="replace"), _digest(result.stdout))
    return found


def saved(path: Path) -> dict[str, str]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return {name: text for name, text in data.items() if isinstance(text, str)} if isinstance(data, dict) else {}


def save(path: Path, identities: dict[str, Identity]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps({name: identity.text for name, identity in sorted(identities.items())}, indent=1)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(text + "\n", encoding="utf-8")
    temporary.replace(path)


def change_lines(before: dict[str, str], now: dict[str, Identity]) -> list[str]:
    lines: list[str] = []
    for name, identity in now.items():
        old = before.get(name)
        if old is None or old == identity.text:
            continue
        lines.append(f"tool {name} changed since it was last recorded:")
        diff = [
            line
            for line in difflib.unified_diff(old.splitlines(), identity.text.splitlines(), lineterm="", n=0)
            if line.startswith(("-", "+")) and not line.startswith(("---", "+++"))
        ]
        lines.extend(f"  {line}" for line in diff[:SHOWN])
        if len(diff) > SHOWN:
            lines.append(f"  ... {len(diff) - SHOWN} more changed lines")
    return lines
