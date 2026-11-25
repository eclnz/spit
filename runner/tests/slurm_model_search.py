from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from typing import Callable, Generic, Iterator, TypeVar

State = TypeVar("State")
Edge = TypeVar("Edge", bound="Step")


@dataclass(frozen=True)
class Step(Generic[State]):
    label: str
    state: State


@dataclass(frozen=True)
class Model(Generic[State, Edge]):
    name: str
    initial: State
    transitions: Callable[[State, str | None], Iterator[Edge]]
    violation: Callable[[State, Edge], str | None]
    faults: tuple[str, ...]


@dataclass(frozen=True)
class Exploration:
    states: int
    edges: int
    problem: str | None = None
    trace: tuple[str, ...] = ()


def explore(model: Model[State, Edge], fault: str | None = None) -> Exploration:
    frontier = deque([model.initial])
    parent: dict[State, tuple[State, str] | None] = {model.initial: None}
    edges = 0
    while frontier:
        current = frontier.popleft()
        for step in model.transitions(current, fault):
            edges += 1
            problem = model.violation(current, step)
            if problem:
                trace = [step.label]
                while (previous := parent[current]) is not None:
                    current, label = previous
                    trace.append(label)
                return Exploration(len(parent), edges, problem, tuple(reversed(trace)))
            if step.state not in parent:
                parent[step.state] = (current, step.label)
                frontier.append(step.state)
    return Exploration(len(parent), edges)
