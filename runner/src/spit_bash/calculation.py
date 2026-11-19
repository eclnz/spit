"""Validate and evaluate the prefix expressions in a DAG's calculation bodies."""

from __future__ import annotations

import math
import operator
from dataclasses import dataclass
from typing import Callable

from .scalars import Read

# Keep in step with the expression parser in SPIT's src/parser/calculation.rs.
OPERATORS: dict[str, tuple[int, Callable[..., float]]] = {
    "+": (2, operator.add),
    "-": (2, operator.sub),
    "*": (2, operator.mul),
    "/": (2, operator.truediv),
    "^": (2, math.pow),
    "neg": (1, operator.neg),
    "exp": (1, math.exp),
    "log": (1, math.log),
    "sqrt": (1, math.sqrt),
    "abs": (1, abs),
    "min": (2, min),
    "max": (2, max),
    "round": (2, round),
}


@dataclass(frozen=True)
class Calculation:
    """An expression with each input port bound to one file."""

    expression: object
    paths: dict[str, str]

    @property
    def reads(self) -> tuple[Read, ...]:
        return tuple(Read(path) for path in dict.fromkeys(self.paths.values()))

    def result(self, values: dict[str, str]) -> str:
        """Evaluate using the validated scalar contents and format one finite decimal."""
        def evaluate(node: object) -> tuple[float, int | None]:
            if isinstance(node, str):
                return float(values[self.paths[node]]), None
            if not isinstance(node, list):
                return float(node), None  # type: ignore[arg-type]
            name, *operands = node
            numbers = [evaluate(operand)[0] for operand in operands]
            places = None
            try:
                if name == "round":
                    if not numbers[1].is_integer() or not 0 <= numbers[1] <= 15:
                        raise ValueError("decimal places must be an integer from 0 to 15")
                    places = int(numbers[1])
                    result = round(numbers[0], places)
                else:
                    result = OPERATORS[name][1](*numbers)
            except (ArithmeticError, ValueError) as exc:
                raise ValueError(f"{name} failed: {exc}") from exc
            if not math.isfinite(result):
                raise ValueError(f"{name} gave a non-finite result")
            return result, places

        result, places = evaluate(self.expression)
        return repr(result) if places is None else f"{result:.{places}f}"


def bind(expression: object, inputs: dict[str, str]) -> Calculation:
    """Validate the DAG's expression and bind each referenced single-file input."""
    paths: dict[str, str] = {}
    pending = [(expression, 1)]
    remaining = 128
    while pending:
        node, depth = pending.pop()
        remaining -= 1
        if remaining < 0 or depth > 64:
            raise ValueError("calculation is too long")
        if isinstance(node, str):
            if node not in inputs:
                raise ValueError(f"calculation input {node!r} must name a single-file input port")
            paths[node] = inputs[node]
        elif type(node) in (float, int):
            try:
                finite = math.isfinite(node)  # type: ignore[arg-type]
            except OverflowError:
                finite = False
            if not finite:
                raise ValueError("calculation constant is not finite")
        elif isinstance(node, list) and node and isinstance(node[0], str) and node[0] in OPERATORS:
            arity = OPERATORS[node[0]][0]
            if len(node) != arity + 1:
                raise ValueError(f"calculation operator {node[0]!r} takes {arity} operands")
            pending.extend((operand, depth + 1) for operand in reversed(node[1:]))
        else:
            raise ValueError("expected a finite number, input port or known operator in calculation")
    return Calculation(expression, paths)


def identify(expression: object, inputs: dict[str, str]) -> object:
    """An already validated expression with port references replaced by work slots."""
    if isinstance(expression, str):
        return inputs[expression]
    if isinstance(expression, list):
        return [expression[0], *(identify(operand, inputs) for operand in expression[1:])]
    return expression
