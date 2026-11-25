from __future__ import annotations

import hashlib
import json
from typing import Iterable

from ..calculation import identify
from ..cause import TOOL, Part
from ..model import Artifact, Check, Work
from ..schema import (
    Artifact as Declared,
    CalculatePart,
    Command,
    CommandPart,
    DirPart,
    HerePart,
    JoinedValuesPart,
    JoinPart,
    ListPart,
    MemberPart,
    PathPart,
    ScalarPart,
    SpitJob,
    StemPart,
)


def slots(item: SpitJob) -> dict[str, str]:
    found: dict[str, str] = {}
    for port, artifacts in enumerate(item.inputs.values()):
        for place, artifact in enumerate(artifacts):
            slot = found.setdefault(artifact.path, f"in{port}.{place}")
            for member in artifact.members or ():
                found.setdefault(f"{artifact.path}/{member.path}", f"{slot}/{member.path}")
    for port, artifact in enumerate(item.outputs.values()):
        found.setdefault(artifact.path, f"out{port}")
    return found


def _digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, separators=(",", ":"), sort_keys=True).encode()).hexdigest()[:16]


def _parts(item: SpitJob, slots: dict[str, str], filtered: bool = False) -> dict[str, object]:

    def of(path: str) -> str:
        return slots.get(path, path)

    def part(value: CommandPart) -> list | str:
        match value:
            case str():
                return value
            case PathPart(path=path):
                return ["path", of(path)]
            case DirPart(dir=folder, of_path=path):
                return ["dir", folder, of(path)]
            case StemPart(stem=stem, of_path=path):
                return ["stem", stem, of(path)]
            case ScalarPart(scalar=path, least=least, greatest=greatest):
                return ["scalar", of(path), least, greatest]
            case JoinPart(join=paths, separator=separator):
                return ["join", [of(path) for path in paths], separator]
            case ListPart(list=paths):
                return ["list", [of(path) for path in paths]]
            case CalculatePart(calculate=calculation):
                inputs = {name: of(artifacts[0].path) for name, artifacts in item.inputs.items() if len(artifacts) == 1}
                return ["calculate", identify(calculation, inputs)]
            case HerePart(here=here):
                return ["here", here]
            case MemberPart() | JoinedValuesPart():
                raise AssertionError("gates resolve a job's member parts before it is identified")

    def command(arguments: Command) -> list:
        return [[part(value) for value in argument] for argument in arguments]

    inputs = [[_input(a, slots) for a in artifacts] for artifacts in item.inputs.values()]
    if filtered and item.collection is not None:
        ports = tuple(item.inputs.values())
        for port in item.collection.ports:
            for entry, artifact in zip(inputs[port], ports[port]):
                entry.append([artifact.entities[name] for name in item.collection.varied])
    return {
        Part.INPUTS.value: inputs,
        Part.OUTPUTS.value: [_output(port, a, item, slots) for port, a in item.outputs.items()],
        Part.COMMANDS.value: [
            [command(c.command), None if c.stdin is None else of(c.stdin), None if c.stdout is None else of(c.stdout)]
            for c in item.commands
        ],
        Part.VERIFY.value: [command(c) for c in item.verify],
    }


def _input(artifact: Declared, slots: dict[str, str]) -> list:
    held = [slots[artifact.path], artifact.kind]
    if artifact.members is None:
        return held
    return [*held, sorted(member.path for member in artifact.members)]


def _output(port: str, artifact: Declared, item: SpitJob, slots: dict[str, str]) -> list:
    held = [slots[artifact.path], artifact.kind]
    seed = (item.seed_folders or {}).get(port)
    return held if seed is None else [*held, "seed", slots[item.inputs[seed][0].path]]


def checks_key(checks: tuple[Check, ...], slots: dict[str, str], shown: dict[str, str] | None = None) -> str | None:
    if not checks:
        return None
    shown = shown or {}
    text = json.dumps(
        [
            [c.side.value, c.name, slots[c.path], [a.replace(shown.get(c.path, c.path), "{@path}") for a in c.command]]
            for c in checks
        ],
        separators=(",", ":"),
    )
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def key(outputs: tuple[Artifact, ...]) -> str:
    return _made([(output.product, output.entities) for output in outputs])


def wire_key(outputs: Iterable[Declared]) -> str:
    return _made([(output.product, tuple(sorted(output.entities.items()))) for output in outputs])


def _made(made: list[tuple[str, tuple[tuple[str, str], ...]]]) -> str:
    return json.dumps(sorted([product, entities] for product, entities in made), separators=(",", ":"))


def work(
    item: SpitJob,
    slots: dict[str, str],
    checks: tuple[Check, ...],
    outputs: tuple[Artifact, ...],
    shown: dict[str, str] | None = None,
    tools: dict[str, str] | None = None,
    roots: tuple[str, str] | None = None,
    *,
    filtered: bool = False,
) -> Work:
    parts = _parts(item, slots, filtered)
    if item.anchored and roots is not None:
        parts[Part.PATHS.value] = {"files": sorted(slots), "root": roots[0], "output_root": roots[1]}
    named = {name: _digest(value) for name, value in parts.items()}
    if tools:
        parts["tools"] = dict(sorted(tools.items()))
        named.update({f"{TOOL}{name}": digest for name, digest in tools.items()})
    return Work(_digest(parts), named, checks_key(checks, slots, shown), key(outputs))
