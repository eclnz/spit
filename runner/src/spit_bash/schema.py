from __future__ import annotations

from pathlib import PurePosixPath
from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict, Field, JsonValue, model_validator

VERSION = 18


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


class Member(SchemaModel):
    id: Annotated[int, Field(ge=0)]
    path: RelativePath


class Artifact(SchemaModel):
    product: NonEmpty
    entities: dict[NonEmpty, str]
    artifact_type: NamedType | VariableType | None = Field(alias="type")
    path: RelativePath
    under: Literal["root", "output_root", "pipeline_folder"]
    id: Annotated[int, Field(ge=0)] | None = None
    kind: Literal["file", "folder", "directory_view"]
    members: list[Member] | None = None

    @model_validator(mode="after")
    def _members(self) -> "Artifact":
        if (self.kind == "directory_view") != (self.members is not None):
            raise ValueError(f"{self.path}: `members` belongs on a directory view, and only there")
        return self


class PathPart(SchemaModel):
    path: RelativePath


class DirPart(SchemaModel):
    dir: str
    of_path: RelativePath = Field(alias="of")


class StemPart(SchemaModel):
    stem: str
    of_path: RelativePath = Field(alias="of")


class HerePart(SchemaModel):
    here: str


class ScalarPart(SchemaModel):
    scalar: RelativePath
    least: float | None = Field(alias="min")
    greatest: float | None = Field(alias="max")


class JoinPart(SchemaModel):
    join: list[RelativePath]
    port: int | None = None
    separator: str


class ListPart(SchemaModel):
    list: list[RelativePath]
    port: int | None = None


class MemberPart(SchemaModel):
    port: Annotated[int, Field(ge=0)]
    offset: Annotated[int, Field(ge=0)]
    member: str | PathPart


class JoinedValuesPart(SchemaModel):
    port: Annotated[int, Field(ge=0)]
    join_values: list[str]
    separator: str


class CalculatePart(SchemaModel):
    calculate: JsonValue


CommandPart = (
    CalculatePart
    | MemberPart
    | JoinedValuesPart
    | str
    | PathPart
    | DirPart
    | StemPart
    | HerePart
    | ScalarPart
    | JoinPart
    | ListPart
)
Argument = Annotated[list[CommandPart], Field(min_length=1)]
Command = Annotated[list[Argument], Field(min_length=1)]


class Tool(SchemaModel):
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
    when: Literal["before", "after"]
    check: NonEmpty
    port: NonEmpty
    path: RelativePath
    command: Command


class PipelineFile(SchemaModel):
    path: NonEmpty
    blob: Annotated[str, Field(pattern=r"^[0-9a-f]{40}$")]


Index = Annotated[int, Field(ge=0)]
Line = Annotated[int, Field(ge=1)]


class CallSite(SchemaModel):
    file: Index | None
    line: Line


class Call(SchemaModel):
    operation: NonEmpty
    instance: NonEmpty
    parent: Index | None
    file: Index | None
    at: CallSite


class Origin(SchemaModel):
    call: Index | None
    file: Index | None
    line: Line


class JobCommand(SchemaModel):
    command: Command
    stdin: RelativePath | None
    stdout: RelativePath | None


class ReviewInput(SchemaModel):
    job: JobId
    port: Annotated[int, Field(ge=0)]


class ReviewTarget(SchemaModel):
    artifact: Artifact
    input: ReviewInput | None


class Review(SchemaModel):
    name: NonEmpty
    file: NonEmpty | None
    dimensions: list[NonEmpty]
    targets: list[ReviewTarget]


class Collection(SchemaModel):
    ports: Annotated[list[Annotated[int, Field(ge=0)]], Field(min_length=1)]
    varied: list[NonEmpty]
    minimum: Annotated[int, Field(ge=1)]


class Screen(SchemaModel):
    when: Literal["before", "after"]
    screen: NonEmpty
    port: NonEmpty
    path: RelativePath
    product_scope: bool
    command: Command


class MissingInput(SchemaModel):
    message: NonEmpty
    member: list[str] | None


class SpitJob(SchemaModel):
    id: JobId
    operation: NonEmpty
    stage: list[str]
    inputs: dict[NonEmpty, list[Artifact]]
    outputs: Annotated[dict[NonEmpty, Artifact], Field(min_length=1)]
    seed_folders: dict[NonEmpty, NonEmpty] | None = None
    collection: Collection | None = None
    missing: list[MissingInput] = Field(default_factory=list)
    depends_on: list[JobId]
    dependents: list[JobId]
    commands: list[JobCommand]
    verify: list[Command]
    checks: list[Check]
    screens: list[Screen] = Field(default_factory=list)
    origin: Origin | None
    cores: Annotated[int, Field(ge=1)]
    memory: Annotated[int, Field(ge=0, le=2**53)] | None
    tools: list[NonEmpty]
    anchored: bool


class SpitDag(SchemaModel):
    version: int
    generator: Generator
    root: str | None
    output_root: str | None
    external_inputs: list[Artifact]
    targets: list[Artifact]
    executables: list[str]
    tools: list[Tool]
    removed: list[Removal]
    left_out: list[LeftOut]
    reviews: list[Review]
    screen_records: dict[NonEmpty, RelativePath] = Field(default_factory=dict)
    pipeline_folder: NonEmpty | None
    pipeline_files: list[PipelineFile]
    calls: list[Call]
    jobs: list[SpitJob]

    @model_validator(mode="before")
    @classmethod
    def _version(cls, data: object) -> object:
        version = data.get("version") if isinstance(data, dict) else None
        if version != VERSION:
            raise ValueError(
                f"spit-bash reads version {VERSION} of the DAG format, not {version}; "
                f"use a spit and spit-bash that match"
            )
        return data
