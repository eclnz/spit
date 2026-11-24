"""What identifies a job's work and its record, whatever its files and things are called."""

from __future__ import annotations

import hashlib
import json
from typing import Iterable

from ..calculation import identify
from ..model import Artifact, Check, Work
from ..schema import Artifact as Declared, CalculatePart, Command, CommandPart, DirPart, JoinPart, ListPart, PathPart, ScalarPart, SpitJob, StemPart

# The work's parts, each hashed on its own. Keep in step with the keys `_parts` gives.
PARTS = ("commands", "verify", "inputs", "outputs", "paths")
# The start of the part that holds one tool's identity, by the tool's name.
TOOL = "tool:"


def slots(item: SpitJob) -> dict[str, str]:
    """Each input and output path's slot, as `Artifact.slot` gives it.

    An artifact is the same work whatever it is called, so a record names a file
    by where the job holds it, not by product or entities.
    """
    found: dict[str, str] = {}
    for port, artifacts in enumerate(item.inputs.values()):
        for place, artifact in enumerate(artifacts):
            slot = found.setdefault(artifact.path, f"in{port}.{place}")
            # A directory view's files are held under the view's slot by their names in it.
            for member in artifact.members or ():
                found.setdefault(f"{artifact.path}/{member.path}", f"{slot}/{member.path}")
    for port, artifact in enumerate(item.outputs.values()):
        found.setdefault(artifact.path, f"out{port}")
    return found


def _digest(value: object) -> str:
    return hashlib.sha256(json.dumps(value, separators=(",", ":"), sort_keys=True).encode()).hexdigest()[:16]


def _parts(item: SpitJob, slots: dict[str, str], filtered: bool = False) -> dict[str, object]:
    """The job's work as the runner identifies it: the shape of what it reads and
    writes, and its commands, with every path replaced by its place in the job.
    Names, storage paths, checks, scheduling and where the job came from are left
    out, so renaming or moving things changes nothing here. What the inputs hold
    is checked against the record's hashes, not here."""

    def of(path: str) -> str:
        return slots.get(path, path)

    def part(value: CommandPart) -> list | str:
        if isinstance(value, str):
            return value
        if isinstance(value, PathPart):
            return ["path", of(value.path)]
        if isinstance(value, DirPart):
            # The tool is given this text, so a change to it is a change to the work.
            return ["dir", value.dir, of(value.of_path)]
        if isinstance(value, StemPart):
            return ["stem", value.stem, of(value.of_path)]
        if isinstance(value, ScalarPart):
            # The number is the input's; the bounds are part of the work.
            return ["scalar", of(value.scalar), value.least, value.greatest]
        if isinstance(value, JoinPart):
            return ["join", [of(path) for path in value.join], value.separator]
        if isinstance(value, ListPart):
            # The list file's own path changes from run to run, and is not part of the work.
            return ["list", [of(path) for path in value.list]]
        if isinstance(value, CalculatePart):
            inputs = {name: of(artifacts[0].path) for name, artifacts in item.inputs.items() if len(artifacts) == 1}
            return ["calculate", identify(value.calculate, inputs)]
        return ["here", value.here]

    def command(arguments: Command) -> list:
        return [[part(value) for value in argument] for argument in arguments]

    inputs = [[_input(a, slots) for a in artifacts] for artifacts in item.inputs.values()]
    if filtered and item.collection is not None:
        ports = tuple(item.inputs.values())
        for port in item.collection.ports:
            for entry, artifact in zip(inputs[port], ports[port]):
                # Two members can have identical bytes while representing different observations.
                entry.append([artifact.entities[name] for name in item.collection.varied])
    # Ports are listed in order, without their names: a renamed operation or port is the same work.
    return {
        "inputs": inputs,
        "outputs": [_output(port, a, item, slots) for port, a in item.outputs.items()],
        "commands": [
            [command(c.command), None if c.stdin is None else of(c.stdin), None if c.stdout is None else of(c.stdout)]
            for c in item.commands
        ],
        "verify": [command(c) for c in item.verify],
    }


def _input(artifact: Declared, slots: dict[str, str]) -> list:
    """An input as the work holds it; a directory view adds the names of its files, which the tool relies on."""
    held = [slots[artifact.path], artifact.kind]
    if artifact.members is None:
        return held
    return [*held, sorted(member.path for member in artifact.members)]


def _output(port: str, artifact: Declared, item: SpitJob, slots: dict[str, str]) -> list:
    """An output as the work holds it; a seeded folder adds the slot of the input it starts as a copy of."""
    held = [slots[artifact.path], artifact.kind]
    seed = (item.seed_folders or {}).get(port)
    return held if seed is None else [*held, "seed", slots[item.inputs[seed][0].path]]


def checks_key(checks: tuple[Check, ...], slots: dict[str, str], shown: dict[str, str] | None = None) -> str | None:
    """What a successful run records of the checks it passed; None without checks.

    `shown` gives the text a path has in a command when it is not the path itself,
    as `load._command` writes it.
    """
    if not checks:
        return None
    shown = shown or {}
    # The tested artifact stands as its slot and its path in the command as a
    # placeholder, so moving the file does not change what was checked.
    text = json.dumps([[c.side.value, c.name, slots[c.path],
                        [a.replace(shown.get(c.path, c.path), "{@path}") for a in c.command]]
                       for c in checks], separators=(",", ":"))
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def key(outputs: tuple[Artifact, ...]) -> str:
    """The state file's key for a job that makes `outputs`."""
    # Job IDs may change when SPIT resolves a different inventory, and paths
    # change when the pipeline's path rules do. What a job makes is its outputs'
    # products and entities, which neither changes.
    return _made([(output.product, output.entities) for output in outputs])


def wire_key(outputs: Iterable[Declared]) -> str:
    """`key` for a job known only by its outputs as the DAG gives them, as a gated job is."""
    # Keep in step with `load._artifact`, which sorts an artifact's entities the same way.
    return _made([(output.product, tuple(sorted(output.entities.items()))) for output in outputs])


def _made(made: list[tuple[str, tuple[tuple[str, str], ...]]]) -> str:
    return json.dumps(sorted([product, entities] for product, entities in made), separators=(",", ":"))


def work(item: SpitJob, slots: dict[str, str], checks: tuple[Check, ...], outputs: tuple[Artifact, ...],
         shown: dict[str, str] | None = None, tools: dict[str, str] | None = None,
         roots: tuple[str, str] | None = None, *, filtered: bool = False) -> Work:
    """The job's `Work`, from its wire form, its slots, and its checks and outputs as loaded, with
    `shown` as `checks_key` takes it, `tools` the digest of each tool the job depends on, by name,
    and `roots` the absolute dataset and output roots, which an anchored job's work holds.
    A filtered collection identifies its selected members even when their bytes match another selection.
    """
    parts = _parts(item, slots, filtered)
    if item.anchored and roots is not None:
        # An anchored job's outputs hold its paths, so where its files are is part of its work.
        parts["paths"] = {"files": sorted(slots), "root": roots[0], "output_root": roots[1]}
    named = {name: _digest(value) for name, value in parts.items()}
    if tools:
        # Only a job that depends on a tool has this part, so a DAG with no tools identifies work as before.
        parts["tools"] = dict(sorted(tools.items()))
        named.update({f"{TOOL}{name}": digest for name, digest in tools.items()})
    # The parts are hashed apart so that a record can say which of them changed.
    return Work(_digest(parts), named, checks_key(checks, slots, shown), key(outputs))
