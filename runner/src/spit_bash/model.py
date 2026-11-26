from __future__ import annotations

from .calculation import Calculation

from dataclasses import dataclass, field
from enum import Enum, IntEnum
from functools import cached_property
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, TypeVar

from .lists import ListRead
from .scalars import Arguments, Read

if TYPE_CHECKING:
    from .tools import Identity
    from .screening import ScreenPlan


T = TypeVar("T")


class DagError(ValueError):
    pass


class Kind(Enum):
    FILE = "file"
    FOLDER = "folder"


@dataclass(frozen=True)
class Artifact:
    slot: str
    path: str
    kind: Kind
    product: str
    entities: tuple[tuple[str, str], ...]


@dataclass(frozen=True)
class Port:
    name: str
    artifacts: tuple[Artifact, ...]


class Side(Enum):
    BEFORE = "before"
    AFTER = "after"


@dataclass(frozen=True)
class Check:
    side: Side
    name: str
    port: str
    path: str
    command: tuple[str, ...]

    def describe(self) -> str:
        side = "input" if self.side is Side.BEFORE else "output"
        return f"check {self.name} on {side} {self.port} {self.path}"


@dataclass(frozen=True)
class Run:
    arguments: Arguments
    stdin: str | None = None
    stdout: str | None = None
    calculation: Calculation | None = None

    def describe(self, index: int, of: int) -> str:
        return "command" if of == 1 else f"command {index}"


@dataclass(frozen=True)
class Work:
    fingerprint: str
    parts: dict[str, str] = field(compare=False)
    checks_key: str | None
    key: str


@dataclass(frozen=True)
class Job:
    id: int
    operation: str
    stage: tuple[str, ...]
    work: Work
    inputs: tuple[Port, ...]
    outputs: tuple[Port, ...]
    depends_on: tuple[int, ...]
    commands: tuple[Run, ...]
    verify: tuple[Arguments, ...]
    checks: tuple[Check, ...] = ()
    scripts: tuple[str, ...] = ()
    check_scripts: tuple[str, ...] = ()
    origin: tuple[str, ...] = ()
    cores: int = 1
    memory: int | None = None
    reads: tuple[Read, ...] = ()
    lists: tuple[ListRead, ...] = ()
    seeds: tuple[tuple[str, str], ...] = ()
    missing: str | None = None

    @cached_property
    def input_artifacts(self) -> tuple[Artifact, ...]:
        return tuple(artifact for port in self.inputs for artifact in port.artifacts)

    @cached_property
    def output_artifacts(self) -> tuple[Artifact, ...]:
        return tuple(artifact for port in self.outputs for artifact in port.artifacts)

    @cached_property
    def input_paths(self) -> tuple[str, ...]:
        return tuple(artifact.path for artifact in self.input_artifacts)

    @cached_property
    def output_paths(self) -> tuple[str, ...]:
        return tuple(artifact.path for artifact in self.output_artifacts)

    @cached_property
    def identities(self) -> dict[str, str]:
        return {artifact.path: artifact.slot for artifact in (*self.input_artifacts, *self.output_artifacts)}

    @property
    def input_paths_by_slot(self) -> dict[str, str]:
        return {artifact.slot: artifact.path for artifact in self.input_artifacts}

    @property
    def output_paths_by_slot(self) -> dict[str, str]:
        return {artifact.slot: artifact.path for artifact in self.output_artifacts}

    def keyed(self, by_path: dict[str, T]) -> dict[str, T]:
        return {self.identities[path]: value for path, value in by_path.items()}

    @property
    def stdout(self) -> str | None:
        return next((run.stdout for run in self.commands if run.stdout is not None), None)

    @property
    def before(self) -> tuple[Check, ...]:
        return tuple(check for check in self.checks if check.side is Side.BEFORE)

    @property
    def after(self) -> tuple[Check, ...]:
        return tuple(check for check in self.checks if check.side is Side.AFTER)


@dataclass(frozen=True)
class Layout:
    folder: Path

    @property
    def state(self) -> Path:
        return self.folder / "state.jsonl"

    @property
    def tools(self) -> Path:
        return self.folder / "tools.json"

    @property
    def sources(self) -> Path:
        return self.folder / "sources.json"

    @property
    def runs(self) -> Path:
        return self.folder / "runs"

    @staticmethod
    def partial(output: Path) -> Path:
        return output.with_name(f".{output.name}.spit-bash-partial")

    @staticmethod
    def previous(output: Path) -> Path:
        return output.with_name(f".{output.name}.spit-bash-previous")

    @staticmethod
    def edited(output: Path, stamp: str) -> Path:
        return output.with_name(f"{output.name}.edited-{stamp}")


@dataclass(frozen=True)
class Target:
    identity: str
    path: str
    in_dataset: bool
    port: tuple[str, str] | None
    rejected: bool
    reason: str | None
    line: int | None
    in_pipeline: bool = False
    entities: tuple[tuple[str, str], ...] = ()
    evidence: str | None = None
    evidence_makers: tuple[int, ...] = ()


@dataclass(frozen=True)
class AppliedVerdict:
    identity: str
    job: int | None
    port: int | None
    verdict: str
    line: int
    reason: str | None


class Admission(IntEnum):
    OPEN = 0
    HELD = 1
    REJECTED = 2


@dataclass(frozen=True)
class GatedJob:
    id: int
    status: Admission
    outputs: tuple[str, ...]
    key: str
    stage: tuple[str, ...] = ()
    contexts: tuple[tuple[str, tuple[tuple[str, str], ...]], ...] = ()
    depends_on: tuple[int, ...] = ()


@dataclass(frozen=True)
class Review:
    name: str
    file: Path | None
    found: bool
    verdicts: int
    unmatched: tuple[int, ...]
    targets: tuple[Target, ...]
    applied: tuple[AppliedVerdict, ...] = ()
    dimensions: tuple[str, ...] = ()
    generated: bool = False


@dataclass(frozen=True)
class Dag:
    root: Path
    jobs: tuple[Job, ...]
    external_inputs: tuple[str, ...]
    executables: tuple[str, ...]
    left_out: tuple[str, ...]
    folders: frozenset[str] = frozenset()
    absolute: dict[str, Path] = field(default_factory=dict, compare=False)
    output_root: Path | None = None
    in_dataset: frozenset[str] = frozenset()
    in_pipeline: frozenset[str] = frozenset()
    pipeline_folder: Path | None = None
    pipeline_files: tuple[tuple[str, Path, str], ...] = ()
    reviews: tuple[Review, ...] = ()
    gated: tuple[GatedJob, ...] = ()
    review_needs: dict[int, set[int]] = field(default_factory=dict, compare=False)
    screens: ScreenPlan | None = field(default=None, compare=False)
    verdict_files: tuple[Path, ...] = ()
    tools: dict[str, "Identity"] = field(default_factory=dict, compare=False)

    @property
    def outputs(self) -> Path:
        return self.root if self.output_root is None else self.output_root

    @property
    def layout(self) -> Layout:
        return Layout(self.outputs / ".spit-bash")

    def base(self, relative: str) -> Path:
        if relative in self.in_pipeline:
            assert self.pipeline_folder is not None, "load_dag refuses a `here` source without a pipeline folder"
            return self.pipeline_folder
        return self.root if relative in self.in_dataset else self.outputs

    def path(self, relative: str) -> Path:
        path = self.absolute.get(relative)
        return self.checked(relative) if path is None else path

    def checked(self, relative: str, use: str | None = None) -> Path:
        base = self.base(relative)
        path = base.joinpath(*PurePosixPath(relative).parts)
        resolved = path.resolve()
        if not resolved.is_relative_to(base):
            raise DagError(_escape(base, relative, resolved, self._which(relative), use))
        return path

    def _which(self, relative: str) -> str:
        if relative in self.in_pipeline:
            return "pipeline"
        return "dataset" if self.base(relative) == self.root else "output"


def _escape(base: Path, relative: str, resolved: Path, which: str, use: str | None) -> str:
    parts = PurePosixPath(relative).parts
    link = next(
        (PurePosixPath(*parts[:end]) for end in range(1, len(parts) + 1) if base.joinpath(*parts[:end]).is_symlink()),
        None,
    )
    if link is None:
        lead = f"{relative} leads to {resolved}"
    elif str(link) == relative:
        lead = f"{relative} is a link to {resolved}"
    else:
        lead = f"{relative} leads through the link {link} to {resolved}"
    lines = [f"{lead}, outside the {which} root {base}."]
    if use is not None:
        lines.append(f"  {use}.")
    lines.append(
        "  spit-bash reads and writes only under its roots, so no link may lead out of one: "
        "move or copy what it links to under the root."
    )
    return "\n".join(lines)
