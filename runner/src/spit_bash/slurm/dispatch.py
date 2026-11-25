from __future__ import annotations

from collections.abc import Iterable
from dataclasses import replace
from pathlib import Path
from typing import Annotated, NamedTuple

from pydantic import AfterValidator, ConfigDict, Field, TypeAdapter

from ..model import DagError
from . import atomic, commands, intent
from .reconcile import ENDED, Task


def _indices(left: tuple[int, ...]) -> tuple[int, ...]:
    if tuple(sorted(set(left))) != left:
        raise ValueError("reservation indices must be sorted and unique")
    return left


class Dispatch(NamedTuple):
    array: Annotated[str, Field(pattern=r"^[0-9]+$")] | None
    left: Annotated[tuple[Annotated[int, Field(ge=0)], ...], Field(min_length=1), AfterValidator(_indices)]
    cancelled: Annotated[tuple[Annotated[int, Field(ge=0)], ...], AfterValidator(_indices)] = ()


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
        atomic.write(path, DISPATCH.dump_json(record))
        self.records[name] = record

    def reserve(self, name: str, slots: int) -> None:
        self._write(name, Dispatch(None, tuple(range(slots))))

    def accept(self, name: str, array: str) -> None:
        self._write(name, Dispatch(array, self.records[name].left))

    def cancel(self, tasks: Iterable[str]) -> None:
        marked: dict[str, set[int]] = {}
        for task in tasks:
            array, _, index = task.partition("_")
            for name, record in self.records.items():
                if record.array == array and index.isdigit() and int(index) in record.left:
                    marked.setdefault(name, set()).add(int(index))
        for name, indices in marked.items():
            record = self.records[name]
            known = tuple(sorted(set(record.cancelled) | indices))
            if known != record.cancelled:
                self._write(name, record._replace(cancelled=known))

    def clear(self, name: str) -> None:
        (self.slurm / f"{name}.dispatch").unlink(missing_ok=True)
        self.records.pop(name, None)

    def reconcile(self, tasks: tuple[Task, ...], *, write: bool = True, complete: bool = True) -> None:
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
            listed = [row for row in rows if row.id == f"{array}_{row.index}"]
            alive = {row.index for row in listed if row.live}
            ended = {row.index for row in listed if not row.live} | (
                set(record.cancelled) - alive if complete else set()
            )
            left = tuple(index for index in record.left if index not in ended)
            if not left:
                if write:
                    self.clear(name)
                    intent.remove(self.slurm, name)
                else:
                    del self.records[name]
            elif (array, left) != (record.array, record.left):
                updated = Dispatch(array, left, tuple(index for index in record.cancelled if index in left))
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


class Accounted(NamedTuple):
    ended: tuple[Task, ...]
    answered: bool


async def terminal_accounting(missing: dict[str, Task]) -> Accounted:
    identities = {id: task.intent for id, task in missing.items() if task.intent is not None}
    if not identities:
        return Accounted((), True)
    states = await commands.accounting(sorted(identities), intents=identities)
    if states is None:
        return Accounted((), False)
    return Accounted(
        tuple(replace(task, state=states[id]) for id, task in missing.items() if states.get(id) in ENDED), True
    )
