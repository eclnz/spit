"""The version 15 SPIT DAG schema, before filesystem checks or execution."""

from __future__ import annotations

from pathlib import PurePosixPath
from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict, Field, model_validator

# The one version of the format spit-bash reads.
VERSION = 15


class SchemaModel(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid", frozen=True)


NonEmpty = Annotated[str, Field(min_length=1)]
JobId = Annotated[int, Field(ge=1)]


def relative_path(value: str) -> str:
    path = PurePosixPath(value)
    if not value or path.is_absolute() or any(part in (".", "..") for part in value.split("/")):
        raise ValueError(f"path must stay within the dataset root: {value}")
    return value


RelativePath = Annotated[str, AfterValidator(relative_path)]


class VariableType(SchemaModel):
    variable: NonEmpty


class NamedType(SchemaModel):
    name: NonEmpty
    args: list[NamedType | VariableType | None]


class Artifact(SchemaModel):
    product: NonEmpty
    entities: dict[NonEmpty, str]
    artifact_type: NamedType | VariableType | None = Field(alias="type")
    path: RelativePath
    # The top-level field holding the folder `path` is relative to.
    under: Literal["root", "output_root"]
    kind: Literal["file", "folder"]


class PathPart(SchemaModel):
    path: RelativePath


class DirPart(SchemaModel):
    dir: str
    of_path: RelativePath = Field(alias="of")


class StemPart(SchemaModel):
    stem: str
    of_path: RelativePath = Field(alias="of")


class HerePart(SchemaModel):
    """The folder of the file declaring the job's operation, relative to the pipeline's folder; "" is that folder."""

    here: str


class ScalarPart(SchemaModel):
    """The number in an input file, read when the job starts, with the least and greatest values allowed."""

    scalar: RelativePath
    least: float | None = Field(alias="min")
    greatest: float | None = Field(alias="max")


CommandPart = str | PathPart | DirPart | StemPart | HerePart | ScalarPart
Argument = Annotated[list[CommandPart], Field(min_length=1)]
Command = Annotated[list[Argument], Field(min_length=1)]


class Tool(SchemaModel):
    """What a tool is on the machine the jobs run on: a declared text, or a probe whose output is its identity."""

    name: NonEmpty
    identity: str | None
    probe: Annotated[list[str], Field(min_length=1)] | None

    @model_validator(mode="after")
    def _one(self) -> "Tool":
        if (self.identity is None) == (self.probe is None):
            raise ValueError(f"tool {self.name} needs exactly one of `identity` and `probe`")
        return self


class Generator(SchemaModel):
    name: NonEmpty
    version: NonEmpty


class Removal(SchemaModel):
    product: str | None
    entities: dict[str, str]
    rule: str
    origin: str | None
    reason: str | None
    found: Annotated[int, Field(ge=0)] | None


class LeftOut(SchemaModel):
    identity: NonEmpty
    reasons: list[str]


class Check(SchemaModel):
    """A check of one artifact, before the command on an input or after it on an output."""

    when: Literal["before", "after"]
    check: NonEmpty
    port: NonEmpty
    path: RelativePath
    command: Command


class PipelineFile(SchemaModel):
    """A file the pipeline was read from, by its git blob id."""

    path: NonEmpty
    blob: Annotated[str, Field(pattern=r"^[0-9a-f]{40}$")]


Index = Annotated[int, Field(ge=0)]
Line = Annotated[int, Field(ge=1)]


class CallSite(SchemaModel):
    file: Index | None
    line: Line


class Call(SchemaModel):
    """A call to an operation carried out by steps, which the DAG holds as its jobs."""

    operation: NonEmpty
    instance: NonEmpty
    parent: Index | None
    file: Index | None
    at: CallSite


class Origin(SchemaModel):
    """Where the step that made a job is written: the line of a file, and the call whose body holds it.

    A step written in the pipeline has no call.
    """

    call: Index | None
    file: Index | None
    line: Line


class JobCommand(SchemaModel):
    """One of a job's commands, which run in order, and the files its streams are connected to."""

    command: Command
    # The input file the command reads on its standard input, and the output
    # file its standard output is written to; null when it does not redirect.
    stdin: RelativePath | None
    stdout: RelativePath | None


class ReviewPort(SchemaModel):
    operation: NonEmpty
    port: NonEmpty


class ReviewTarget(SchemaModel):
    """An artifact a review withheld when SPIT made the plan, which a person inspects to give a verdict."""

    artifact: Artifact
    # The input port the review is on, null for a review on a source or an output.
    port: ReviewPort | None
    # "fail" for a rejection, null for an artifact no row judges yet.
    verdict: Literal["fail"] | None
    reason: str | None
    line: Line | None
    # The outputs not planned because of it, and those of planned jobs that take a `many` input without it.
    withholds: list[NonEmpty]
    shrinks: list[NonEmpty]


class Review(SchemaModel):
    name: NonEmpty
    # The review's file as an absolute path, null when the recipe names none.
    file: NonEmpty | None
    found: bool
    verdicts: Annotated[int, Field(ge=0)]
    unmatched: list[Line]
    targets: list[ReviewTarget]


class SpitJob(SchemaModel):
    id: JobId
    operation: NonEmpty
    stage: list[str]
    inputs: dict[NonEmpty, list[Artifact]]
    outputs: Annotated[dict[NonEmpty, Artifact], Field(min_length=1)]
    depends_on: list[JobId]
    dependents: list[JobId]
    # Empty for an operation with no command.
    commands: list[JobCommand]
    verify: list[Command]
    checks: list[Check]
    # Null for a step SPIT did not read from a file.
    origin: Origin | None
    # What the job needs while its commands run: cores, 1 when its operation
    # declares none, and memory in bytes, null when it claims none.
    cores: Annotated[int, Field(ge=1)]
    memory: Annotated[int, Field(ge=0, le=2**53)] | None
    # The tools the job depends on, by name, in the order of `SpitDag.tools`.
    tools: list[NonEmpty]


class SpitDag(SchemaModel):
    version: int
    generator: Generator
    root: str | None
    # The folder the jobs' outputs are under and commands run from; `root` when the
    # pipeline names no other, null when neither is known.
    output_root: str | None
    external_inputs: list[Artifact]
    targets: list[Artifact]
    executables: list[str]
    tools: list[Tool]
    removed: list[Removal]
    left_out: list[LeftOut]
    reviews: list[Review]
    # The absolute folder of the pipeline's own file, null when SPIT read the pipeline from no file.
    pipeline_folder: NonEmpty | None
    pipeline_files: list[PipelineFile]
    calls: list[Call]
    jobs: list[SpitJob]

    @model_validator(mode="before")
    @classmethod
    def _version(cls, data: object) -> object:
        """Name the version first, since a DAG of another one fails on fields it lacks or adds."""
        version = data.get("version") if isinstance(data, dict) else None
        if version != VERSION:
            raise ValueError(f"spit-bash reads version {VERSION} of the DAG format, not {version}; "
                             f"use a spit and spit-bash that match")
        return data
