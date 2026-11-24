from __future__ import annotations

from dataclasses import replace
from pathlib import Path
from typing import Annotated, NamedTuple

from pydantic import AfterValidator, ConfigDict, Field, TypeAdapter

from ..model import DagError
from . import commands, intent
from .reconcile import ENDED, Task


def _indices(left: tuple[int, ...]) -> tuple[int, ...]:
    if tuple(sorted(set(left))) != left:
        raise ValueError("reservation indices must be sorted and unique")
    return left


class Dispatch(NamedTuple):
    array: Annotated[str, Field(pattern=r"^[0-9]+$")] | None
    left: Annotated[tuple[Annotated[int, Field(ge=0)], ...], Field(min_length=1), AfterValidator(_indices)]


DISPATCH = TypeAdapter(Dispatch, config=ConfigDict(strict=True))


class Dispatches:
    def __init__(self, slurm: Path) -> None:
        self.slurm = slurm
        self.records: dict[str, Dispatch] = {}
        try:
            paths = list(slurm.iterdir())
        except FileNotFoundError:
            return
        except OSError as exc:
            raise DagError(f"cannot read Slurm reservations in {slurm}: {exc}") from exc
        for path in paths:
            if path.suffix != ".dispatch":
                continue
            try:
                self.records[path.stem] = DISPATCH.validate_json(path.read_bytes())
            except (OSError, ValueError) as exc:
                raise DagError(f"cannot read Slurm reservation {path}: {exc}") from exc

    def _write(self, name: str, record: Dispatch) -> None:
        path = self.slurm / f"{name}.dispatch"
        intent.atomic(path, DISPATCH.dump_json(record))
        self.records[name] = record

    def reserve(self, name: str, slots: int) -> None:
        self._write(name, Dispatch(None, tuple(range(slots))))

    def accept(self, name: str, array: str) -> None:
        self._write(name, Dispatch(array, self.records[name].left))

    def clear(self, name: str) -> None:
        (self.slurm / f"{name}.dispatch").unlink(missing_ok=True)
        self.records.pop(name, None)

    def reconcile(self, tasks: tuple[Task, ...], *, write: bool = True) -> None:
        by_intent: dict[str, list[Task]] = {}
        for task in tasks:
            if task.intent in self.records:
                by_intent.setdefault(task.intent, []).append(task)
        for name, record in list(self.records.items()):
            rows = by_intent.get(name, [])
            arrays = {row.id.partition("_")[0] for row in rows if row.index in record.left}
            array = record.array
            if array is None and len(arrays) == 1:
                array = next(iter(arrays))
            if array is None:
                continue
            ended = {row.index for row in rows if row.id == f"{array}_{row.index}" and not row.live}
            left = tuple(index for index in record.left if index not in ended)
            if not left:
                if write:
                    self.clear(name)
                    intent.remove(self.slurm, name)
                else:
                    del self.records[name]
            elif (array, left) != (record.array, record.left):
                updated = Dispatch(array, left)
                if write:
                    self._write(name, updated)
                else:
                    self.records[name] = updated

    @property
    def unknown(self) -> tuple[str, ...]:
        return tuple(sorted(name for name, record in self.records.items() if record.array is None))

    def tasks(self) -> dict[str, Task]:
        return {
            f"{record.array}_{index}": Task(f"{record.array}_{index}", name, index, "")
            for name, record in self.records.items()
            if record.array is not None
            for index in record.left
        }

    def missing(self, tasks: tuple[Task, ...]) -> dict[str, Task]:
        present = {task.id for task in tasks}
        return {id: task for id, task in self.tasks().items() if id not in present}


async def terminal_accounting(missing: dict[str, Task]) -> tuple[Task, ...]:
    identities = {id: task.intent for id, task in missing.items() if task.intent is not None}
    states = await commands.accounting(sorted(identities), intents=identities) if identities else None
    return tuple(
        replace(task, state=states[id])
        for id, task in missing.items()
        if states is not None and states.get(id) in ENDED
    )
