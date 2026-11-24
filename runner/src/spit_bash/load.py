"""Turn a validated SPIT document into runnable jobs."""

from __future__ import annotations

import gc
import hashlib
import json
import os
from dataclasses import replace
from pathlib import Path, PurePosixPath
from typing import Iterable, Iterator, TextIO

from pydantic import ValidationError

from . import calculation, gates, lists, scalars, schema, tools
from .model import Artifact, Check, Dag, DagError, GatedJob, Job, Kind, Port, Run, Side
from .reuse import identity
from .screening import BoundScreen, ScreenPlan, check_record_locations, screen_attachments
from .schema import (CalculatePart, Command, DirPart, HerePart, JobCommand, JoinPart, ListPart, PathPart, ScalarPart,
                     SpitDag, SpitJob,
                     StemPart)


def _here(part: HerePart, folder: str | None) -> str:
    if folder is None:
        raise DagError("a command uses the pipeline's folder, but `pipeline_folder` is null")
    inside = PurePosixPath(part.here)
    if inside.is_absolute() or ".." in inside.parts:
        raise DagError(f"`here` names {part.here}, which is not a folder under the pipeline's")
    return str(PurePosixPath(folder) / inside)


def _command(command: Command, folder: str | None = None, shown: dict[str, str] | None = None) -> tuple[str, ...]:
    """A command's arguments as text; `folder` is the pipeline's, which a `here` part is under.

    `shown` gives the text of each path under a dataset root apart from the
    output root, which commands run from: its absolute path.
    """
    shown = shown or {}
    arguments: list[str] = []
    for argument in command:
        parts: list[str | scalars.Read | lists.ListRead] = []
        for part in argument:
            if isinstance(part, CalculatePart):
                raise DagError("a calculation must occupy a job's entire command body")
            if isinstance(part, str):
                parts.append(part)
            elif isinstance(part, JoinPart):
                parts.append(_join(part, shown))
            elif isinstance(part, ListPart):
                parts.append(lists.ListRead(tuple(part.list)))
            elif isinstance(part, PathPart):
                parts.append(shown.get(part.path, part.path))
            elif isinstance(part, DirPart):
                parts.append(_dir(part, shown))
            elif isinstance(part, StemPart):
                parts.append(part.stem)
            elif isinstance(part, HerePart):
                parts.append(_here(part, folder))
            elif isinstance(part, ScalarPart):
                parts.append(scalars.Read(part.scalar, part.least, part.greatest))
        if any(isinstance(part, (scalars.Read, lists.ListRead)) for part in parts):
            arguments.append(scalars.Reading(tuple(parts)))
        else:
            arguments.append("".join(parts))  # type: ignore[arg-type]
    if not arguments[0]:
        raise DagError("command has no executable")
    if any(isinstance(part, (JoinPart, ListPart)) for part in command[0]):
        raise DagError("command starts with a `join` or `list` part, not a program")
    if isinstance(arguments[0], scalars.Reading):
        raise DagError("command starts with a `scalar` part, not a program")
    return tuple(arguments)


def _scripts(commands: Iterable[Command], folder: str | None) -> tuple[str, ...]:
    """The files and folders that arguments starting with a `here` part name, in order, each once."""
    found: dict[str, None] = {}
    for command in commands:
        for argument in command:
            if not argument or not isinstance(argument[0], HerePart):
                continue
            # The path ends where the argument stops being plain text, as at a scalar.
            tail = ""
            for part in argument[1:]:
                if not isinstance(part, str):
                    break
                tail += part
            found[_here(argument[0], folder) + tail] = None
    return tuple(found)


def _join(part: JoinPart, shown: dict[str, str]) -> str:
    """A `join` part's text: its paths, as commands name them, joined with its separator.

    A path holding the separator could not be told apart from two, so it is refused. Keep in step with
    `collection` in SPIT's `src/bind.rs`, which makes the same check before the paths are absolute.
    """
    paths = [shown.get(path, path) for path in part.join]
    if part.separator:
        holding = next((path for path in paths if part.separator in path), None)
        if holding is not None:
            raise DagError(f"{holding} holds the join separator {part.separator!r}, "
                           f"so a tool could not split the joined paths apart")
    return part.separator.join(paths)


def _dir(part: DirPart, shown: dict[str, str]) -> str:
    """A `dir` part's text: the folder as written, or under a dataset root apart from the output root, its absolute path."""
    full = shown.get(part.of_path)
    # SPIT writes a `dir` part as the folder its file is in.
    return part.dir if full is None else str(PurePosixPath(full).parent)


def _artifact(declared: schema.Artifact, slots: dict[str, str]) -> Artifact:
    # Keep in step with `identity.wire_key`, which sorts a gated job's entities the same way.
    return Artifact(slots[declared.path], declared.path, Kind(declared.kind), declared.product,
                    tuple(sorted(declared.entities.items())))


def _ports(item: SpitJob, slots: dict[str, str], by_id: dict[int, schema.Artifact]
           ) -> tuple[tuple[Port, ...], tuple[Port, ...]]:
    """The job's input ports and output ports, in DAG order; a directory view stands as its files."""
    inputs = tuple(Port(name, tuple(held for a in artifacts for held in _held(a, slots, by_id)))
                   for name, artifacts in item.inputs.items())
    outputs = tuple(Port(name, (_artifact(a, slots),)) for name, a in item.outputs.items())
    return inputs, outputs


def _held(declared: schema.Artifact, slots: dict[str, str], by_id: dict[int, schema.Artifact]) -> Iterator[Artifact]:
    """The artifacts a job reads for one it names: the artifact, or each file of a directory view.

    A view is read only through its files, so they are what the job waits for, checks and identifies.
    """
    if declared.members is None:
        yield _artifact(declared, slots)
        return
    for member in declared.members:
        yield _artifact(by_id[member.id], slots)


def _check_views(document: SpitDag) -> dict[int, schema.Artifact]:
    """Each artifact a job writes, by id, once every directory view is checked to name only files jobs
    write, each at its path inside the view."""
    by_id = {artifact.id: artifact for job in document.jobs for artifact in job.outputs.values()
             if artifact.id is not None}
    for view in _artifacts(document):
        for member in view.members or ():
            found = by_id.get(member.id)
            if found is None or found.kind != "file":
                raise DagError(f"directory view {view.path} names artifact {member.id}, "
                               f"which is no file a job writes")
            if found.path != f"{view.path}/{member.path}" or found.under != view.under:
                raise DagError(f"directory view {view.path} names {member.path}, "
                               f"but artifact {member.id} is at {found.path}")
    return by_id


def _seeds(item: SpitJob) -> tuple[tuple[str, str], ...]:
    """Each output folder the job starts as a copy of an input folder, as (output path, input path)."""
    if item.missing:
        return ()
    seeds = []
    for output, port in (item.seed_folders or {}).items():
        written = item.outputs.get(output)
        if written is None or written.kind != "folder":
            raise DagError(f"job {item.id}: `seed_folders` names {output}, which is not one of its output folders")
        read = item.inputs.get(port, [])
        if len(read) != 1 or read[0].kind != "folder":
            raise DagError(f"job {item.id}: `seed_folders` seeds {output} from {port}, "
                           f"which is not an input of one folder")
        seeds.append((written.path, read[0].path))
    return tuple(seeds)


def _artifacts(document: SpitDag) -> Iterator[schema.Artifact]:
    """Every artifact the document names, wherever it appears."""
    yield from document.external_inputs
    yield from document.targets
    for job in document.jobs:
        for port in job.inputs.values():
            yield from port
        yield from job.outputs.values()


def _origin(document: SpitDag, job: SpitJob) -> tuple[str, ...]:
    """The body step that made `job`, then each call it is nested in, as lines to show."""
    if job.origin is None:
        return ()
    files = document.pipeline_files
    calls = document.calls

    def place(file: int | None, line: int) -> str:
        return f"{files[file].path} line {line}" if file is not None else f"line {line}"

    lines = [f"step at {place(job.origin.file, job.origin.line)}"]
    if job.origin.call is None:
        return tuple(lines)
    call = calls[job.origin.call]
    while True:
        lines.append(f"in `{call.instance} = {call.operation}(...)` at {place(call.at.file, call.at.line)}")
        if call.parent is None:
            return tuple(lines)
        call = calls[call.parent]


def _check_origins(document: SpitDag) -> None:
    """Every reference into the pipeline's files and calls is in range."""
    files = len(document.pipeline_files)
    calls = document.calls
    for position, call in enumerate(calls):
        if any(file is not None and file >= files for file in (call.file, call.at.file)):
            raise DagError(f"call {position} names a file that `pipeline_files` does not hold")
        # A call's parent comes before it, so following parents ends.
        if call.parent is not None and call.parent >= position:
            raise DagError(f"call {position} names a parent that is not an earlier call")
    for job in document.jobs:
        if job.origin is None:
            continue
        if job.origin.call is not None and job.origin.call >= len(calls):
            raise DagError(f"job {job.id}: `origin` names a call that `calls` does not hold")
        if job.origin.file is not None and job.origin.file >= files:
            raise DagError(f"job {job.id}: `origin` names a file that `pipeline_files` does not hold")


def _check_streams(job: SpitJob, folders: frozenset[str]) -> None:
    """Each command's `stdin` names one of the job's input files, and its `stdout` one of its
    output files; one command at most has a `stdout`."""
    if sum(command.stdout is not None for command in job.commands) > 1:
        raise DagError(f"job {job.id}: more than one of its commands has a `stdout`")
    inputs = [artifact for port in job.inputs.values() for artifact in port]
    outputs = list(job.outputs.values())
    for command in job.commands:
        for field_name, path, ports, side in (
            ("stdin", command.stdin, inputs, "inputs"),
            ("stdout", command.stdout, outputs, "outputs"),
        ):
            if path is None:
                continue
            if not any(artifact.path == path for artifact in ports):
                raise DagError(f"job {job.id}: `{field_name}` names {path}, which is not one of its {side}")
            if path in folders:
                raise DagError(f"job {job.id}: `{field_name}` names {path}, which is a folder, not a file")


def _check_reads(job: SpitJob, reads: tuple[scalars.Read, ...], folders: frozenset[str]) -> None:
    """Each `scalar` part names one of the job's input files."""
    inputs = {artifact.path for port in job.inputs.values() for artifact in port}
    for read in reads:
        if read.path not in inputs:
            raise DagError(f"job {job.id}: a `scalar` part names {read.path}, which is not one of its inputs")
        if read.path in folders:
            raise DagError(f"job {job.id}: a `scalar` part names {read.path}, which is a folder, not a file")


def _check_collections(job: SpitJob) -> None:
    """Each `join` and `list` part names the job's input files, and is in a command or a `verify`
    command, not a check, which reads one artifact."""
    inputs = {artifact.path for port in job.inputs.values() for artifact in port}
    for command in (*(run.command for run in job.commands), *job.verify):
        for part in (part for argument in command for part in argument):
            members = part.join if isinstance(part, JoinPart) else part.list if isinstance(part, ListPart) else ()
            outside = next((path for path in members if path not in inputs), None)
            if outside is not None:
                raise DagError(f"job {job.id}: a `join` or `list` part names {outside}, which is not one of its inputs")
    if any(isinstance(part, (JoinPart, ListPart)) for check in job.checks for argument in check.command
           for part in argument):
        raise DagError(f"job {job.id}: a check has a `join` or `list` part, but a check reads only its artifact")


def _run(job: SpitJob, command: JobCommand, folder: str | None, shown: dict[str, str]) -> Run:
    match command.command:
        case [[CalculatePart(calculate=expression)]]:
            if len(job.commands) != 1 or command.stdin is not None or command.stdout is None or len(job.outputs) != 1:
                raise DagError(f"job {job.id}: a calculation needs one file output and no other command or stdin")
            inputs = {name: artifacts[0].path for name, artifacts in job.inputs.items()
                      if len(artifacts) == 1 and artifacts[0].kind == "file"}
            try:
                body = calculation.bind(expression, inputs)
            except ValueError as exc:
                raise DagError(f"job {job.id}: {exc}") from exc
            return Run(("calculate", json.dumps(expression, separators=(",", ":"))), stdout=command.stdout,
                       calculation=body)
    return Run(_command(command.command, folder, shown), command.stdin, command.stdout)


def _absolute(dag: Dag, uses: dict[str, str]) -> dict[str, Path]:
    """Each path joined to its root, with the check `Dag.checked` makes: resolved, it stays under the root.

    Keep in step with `Dag.checked`, which gives the error for a path that leads out. Paths share a few
    folders, so each folder is resolved once, and a file needs resolving only when it is itself a link.
    """
    folders: dict[tuple[str, str], str] = {}
    absolute: dict[str, Path] = {}
    for relative, use in uses.items():
        base = str(dag.base(relative))
        under = base.rstrip("/") + "/"
        joined = under + relative
        if os.path.islink(joined):
            real = os.path.realpath(joined)
        else:
            folder, _, name = relative.rpartition("/")
            real_folder = folders.get((base, folder))
            if real_folder is None:
                real_folder = folders[base, folder] = os.path.realpath(under + folder)
            real = f"{real_folder.rstrip('/')}/{name}"
        if real != base and not real.startswith(under):
            # Resolved again, link by link, to say which link leads out and where.
            absolute[relative] = dag.checked(relative, use)
            continue
        absolute[relative] = Path(joined)
    return absolute


def load_dag(source: TextIO, root_override: str | None = None, *, bypass_verdicts: bool = False) -> Dag:
    """The DAG `source` holds, checked, with its root from `root_override` when given."""
    # Loading makes millions of objects, nearly all kept for as long as the DAG is. With the cyclic collector
    # on, each of its passes walks them all, which more than doubles the time to load; what little is garbage
    # waits for the first pass after.
    enabled = gc.isenabled()
    gc.disable()
    try:
        return _load(source, root_override, bypass_verdicts)
    finally:
        if enabled:
            gc.enable()


def _load(source: TextIO, root_override: str | None, bypass_verdicts: bool) -> Dag:
    try:
        document = SpitDag.model_validate_json(source.read())
    except (ValidationError, UnicodeError) as exc:
        raise DagError(f"invalid SPIT DAG: {exc}") from exc
    root_value = root_override if root_override is not None else document.root
    if not root_value:
        raise DagError("DAG has no root; pass --root DIRECTORY")
    root = Path(root_value).expanduser().resolve()
    if not root.is_dir():
        raise DagError(f"dataset root is not a directory: {root}")
    # Outputs go under the dataset root unless the DAG names another folder; a
    # root given on the command line moves them with it.
    output_root = None
    if document.output_root is not None and document.output_root != document.root:
        output_root = Path(document.output_root).expanduser().resolve()
        if output_root == root:
            output_root = None

    _check_origins(document)
    by_id = _check_views(document)
    potential = document
    producers_of = gates.producers_of(document.jobs)
    attachments = {} if bypass_verdicts else screen_attachments(document)
    screened = {scope for each in attachments.values() for scope, _, _ in each}
    reviews = (gates.ReviewDecisions({}, (), {}) if bypass_verdicts or not document.reviews
               else gates.read_reviews(document, screened))
    pending = {"before": set(), "after": set()}
    for job_id, entries in attachments.items():
        for scope, screen, _ in entries:
            if scope not in reviews.overrides:
                pending[screen.when].add(job_id)
    effective = gates.effective(document, reviews.scopes, producers_of, pending["before"], pending["after"])
    document = effective.document
    # Probes run before any job, on the machine the jobs run on, once for the whole DAG.
    identities = tools.identify(document.tools, root) if document.tools else {}
    folders = frozenset(artifact.path for artifact in _artifacts(potential) if artifact.kind == "folder")
    for artifact in _artifacts(potential):
        if (artifact.kind == "folder") != (artifact.path in folders):
            raise DagError(f"path is both a file and a folder: {artifact.path}")
    unders: dict[str, str] = {}
    for artifact in _artifacts(potential):
        if unders.setdefault(artifact.path, artifact.under) != artifact.under:
            raise DagError(f"path is under both roots: {artifact.path}")
    in_dataset = frozenset(path for path, under in unders.items() if under == "root") if output_root else frozenset()
    in_pipeline = frozenset(path for path, under in unders.items() if under == "pipeline_folder")
    pipeline_folder = Path(document.pipeline_folder).resolve() if document.pipeline_folder is not None else None
    if in_pipeline and (pipeline_folder is None or not pipeline_folder.is_dir()):
        raise DagError(f"the DAG reads {min(in_pipeline)} from the pipeline's folder, "
                       f"which is not a directory: {document.pipeline_folder}")
    # Commands run from the output root, so a path under another root is given whole.
    shown = {path: str(root.joinpath(*PurePosixPath(path).parts)) for path in in_dataset}
    if pipeline_folder is not None:
        shown.update((path, str(pipeline_folder.joinpath(*PurePosixPath(path).parts))) for path in in_pipeline)
    external_paths = tuple(artifact.path for artifact in document.external_inputs)
    external_set = set(external_paths)
    jobs = []
    seen_ids: set[int] = set()
    producers: dict[str, int] = {}
    potential_sizes = {
        job.id: sum(map(len, job.inputs.values())) for job in potential.jobs if job.collection is not None
    } if not bypass_verdicts and (potential.reviews or attachments) else {}

    def materialize(item: SpitJob) -> Job:
        _check_streams(item, folders)
        seeds = _seeds(item)
        if item.missing and item.seed_folders:
            item = item.model_copy(update={"seed_folders": None})
        unknown = [name for name in item.tools if name not in identities]
        if unknown:
            raise DagError(f"job {item.id} names tool {unknown[0]}, which `tools` does not list")
        slots = identity.slots(item)
        inputs, produced = _ports(item, slots, by_id)
        commands = tuple(_run(item, command, document.pipeline_folder, shown) for command in item.commands)
        verify = tuple(_command(command, document.pipeline_folder, shown) for command in item.verify)
        scripts = _scripts((*(command.command for command in item.commands), *item.verify), document.pipeline_folder)
        check_scripts = _scripts((check.command for check in item.checks), document.pipeline_folder)
        reads = scalars.reads((*(run.arguments for run in commands), *verify))
        for run in commands:
            if run.calculation is not None:
                reads = tuple(dict.fromkeys((*reads, *run.calculation.reads)))
        _check_reads(item, reads, folders)
        _check_collections(item)
        checks = tuple(
            Check(Side(check.when), check.check, check.port, check.path,
                  _command(check.command, document.pipeline_folder, shown))
            for check in item.checks
        )
        origin = _origin(document, item)
        # Before the checks key, which names each checked path by its slot.
        for check in item.checks:
            ports = item.inputs if check.when == "before" else {k: [v] for k, v in item.outputs.items()}
            if not any(artifact.path == check.path for artifact in ports.get(check.port, ())):
                raise DagError(f"job {item.id}: check {check.check} names {check.path}, "
                               f"which is not on its {'input' if check.when == 'before' else 'output'} {check.port}")
        filtered = item.id in potential_sizes and sum(map(len, item.inputs.values())) < potential_sizes[item.id]
        return Job(
            id=item.id,
            operation=item.operation,
            stage=tuple(item.stage),
            work=identity.work(item, slots, checks, tuple(a for port in produced for a in port.artifacts), shown,
                               {name: identities[name].digest for name in item.tools},
                               (str(root), str(output_root or root)), filtered=filtered),
            inputs=inputs,
            outputs=produced,
            depends_on=tuple(item.depends_on),
            commands=commands,
            verify=verify,
            checks=checks,
            scripts=scripts,
            check_scripts=check_scripts,
            origin=origin,
            cores=item.cores,
            memory=item.memory,
            reads=reads,
            lists=lists.found((*(run.arguments for run in commands), *verify)),
            seeds=seeds,
            missing=item.missing[0].message if item.missing else None,
        )

    for item in document.jobs:
        if item.id in seen_ids:
            raise DagError(f"duplicate job id: {item.id}")
        outputs = tuple(artifact.path for artifact in item.outputs.values())
        if len(item.depends_on) != len(set(item.depends_on)) or any(dep not in seen_ids for dep in item.depends_on):
            raise DagError(f"job {item.id} dependencies must name earlier jobs once each")
        for path in outputs:
            if path in in_pipeline:
                raise DagError(f"job {item.id} writes {path} under the pipeline's folder, which jobs only read")
            if path in producers or path in external_set:
                raise DagError(f"multiple jobs or sources claim path: {path}")
            producers[path] = item.id
        jobs.append(materialize(item))
        seen_ids.add(item.id)

    for job in jobs:
        expected = {producers[path] for path in job.input_paths if path in producers}
        if set(job.depends_on) != expected:
            raise DagError(f"job {job.id} dependencies do not match its input producers")
        for path in job.input_paths:
            if path not in producers and path not in external_set:
                raise DagError(f"job {job.id} input is neither produced nor external: {path}")
    screens = None
    if attachments:
        before, after = {}, {}
        reviewed = []
        records = potential.screen_records
        active = {job.id for job in document.jobs}
        for job_id, entries in attachments.items():
            for scope, declared, artifact in entries:
                if job_id not in active and scope not in reviews.overrides:
                    continue
                command = _command(declared.command, document.pipeline_folder, shown)
                bound = BoundScreen(scope, declared.port, declared.screen, command, artifact,
                                    records[artifact.product])
                if scope in reviews.overrides:
                    reviewed.append(bound)
                else:
                    (before if declared.when == "before" else after).setdefault(job_id, []).append(bound)
        if before or after or reviewed:
            screens = ScreenPlan(
                {job.id: job for job in potential.jobs if job.id in effective.affected},
                before, after, tuple(reviewed), effective.affected, reviews, dict(effective.excluded),
                producers_of, materialize,
            )
    excluded = dict(effective.excluded)
    review_reports = tuple(replace(report, targets=tuple(
        target for target in report.targets if producers_of.paths.get(target.path) not in excluded
    )) for report in reviews.reports)
    dag = Dag(
        root,
        tuple(jobs),
        external_paths,
        tuple(document.executables),
        tuple(item.identity for item in document.left_out),
        folders,
        output_root=output_root,
        in_dataset=in_dataset,
        in_pipeline=in_pipeline,
        pipeline_folder=pipeline_folder,
        pipeline_files=_pipeline_files(document),
        reviews=review_reports,
        gated=tuple(GatedJob(job.id, excluded[job.id], tuple(artifact.path for artifact in job.outputs.values()),
                             identity.wire_key(job.outputs.values()))
                    for job in potential.jobs if job.id in excluded) if excluded else (),
        screens=screens,
        verdict_files=(*(Path(review.file).resolve() for review in potential.reviews if review.file is not None),
                       *((output_root or root) / record for record in potential.screen_records.values())),
        tools=identities,
    )
    dag.absolute.update(_absolute(dag, _uses(external_set, jobs)))
    if not bypass_verdicts:
        check_record_locations(potential, dag)
    return dag


def _pipeline_files(document: SpitDag) -> tuple[tuple[str, Path, str], ...]:
    """Each file the pipeline was read from, with where it is now and its blob id when the DAG was written."""
    if document.pipeline_folder is None:
        return ()
    folder = Path(document.pipeline_folder)
    return tuple((file.path, folder.joinpath(*PurePosixPath(file.path).parts), file.blob)
                 for file in document.pipeline_files)


def blob_id(data: bytes) -> str:
    """A file's git blob id, as `git hash-object` prints it and SPIT records it in `pipeline_files`."""
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def changed_pipeline_files(dag: Dag) -> list[str]:
    """The pipeline's files that differ from those the DAG was written from, each as its path from the
    pipeline's folder, with `(missing)` after one that is gone."""
    changed = []
    for shown, path, blob in dag.pipeline_files:
        try:
            if blob_id(path.read_bytes()) != blob:
                changed.append(shown)
        except OSError:
            changed.append(f"{shown} (missing)")
    return changed


def _uses(sources: set[str], jobs: list[Job]) -> dict[str, str]:
    """Each artifact's path, in DAG order, with the first job that reads or writes it, for an error to name."""
    uses: dict[str, str] = {}
    for job in jobs:
        for path in job.input_paths:
            uses.setdefault(path, f"Job {job.id} ({job.operation}) reads it{' as a source' if path in sources else ''}")
        for path in job.output_paths:
            uses.setdefault(path, f"Job {job.id} ({job.operation}) writes it")
    for path in sorted(sources - uses.keys()):
        uses[path] = "It is a source no job reads"
    return uses
