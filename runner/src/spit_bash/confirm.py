"""The jobs a run would replace outputs of that took long to make, which an interactive run asks about first."""

from __future__ import annotations

from collections import Counter

from .cause import Why
from .plan import Decision, Status
from .progress import duration
from .state.record import Record

# The default of `run --ask-over`: two minutes.
ASK_OVER = 120.0

UNITS = {"s": 1, "m": 60, "h": 3600}


def parse(text: str) -> float:
    """A length of time as `--ask-over` takes it: a whole number, and s, m or h, as in `90s`, `2m` or `1h`.

    Raises `ValueError` for anything else.
    """
    unit = text[-1:].lower()
    number = text[:-1]
    if unit not in UNITS or not (number.isascii() and number.isdigit()):
        raise ValueError(f"expected a time such as 90s, 2m or 1h, not {text!r}")
    return float(int(number) * UNITS[unit])


def costly(decisions: tuple[Decision, ...], records: dict[str, Record], over: float) -> list[tuple[Decision, float]]:
    """Each job planned to run that replaces an output on disk, whose record says it took at least `over` seconds,
    with those seconds. A forced job is left out: `--force` already asked for it."""
    found = []
    for decision in decisions:
        if decision.status is not Status.RUN or not decision.replaces or decision.cause.why is Why.FORCED:
            continue
        record = records.get(decision.job.work.key)
        if record is not None and record.seconds is not None and record.seconds >= over:
            found.append((decision, record.seconds))
    return found


def lines(found: list[tuple[Decision, float]]) -> list[str]:
    """What the question lists: how many jobs, and how long their outputs took, then each operation with the time
    its jobs took. Why each reruns is in the plan's listing just before."""
    took: Counter[str] = Counter()
    count: Counter[str] = Counter()
    for decision, seconds in found:
        took[decision.job.operation] += seconds
        count[decision.job.operation] += 1
    noun = "job" if len(found) == 1 else "jobs"
    total = duration(sum(seconds for _, seconds in found))
    out = [f"{len(found)} {noun} will replace outputs that took {total} to make:"]
    for operation, seconds in took.items():
        times = f" x{count[operation]}" if count[operation] > 1 else ""
        out.append(f"  {operation}{times} ({duration(seconds)})")
    return out
