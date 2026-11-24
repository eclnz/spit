from __future__ import annotations

import json
from collections import Counter

from ..model import Dag
from ..plan import Decision, Status
from .causes import decided


def document(
    dag: Dag,
    decisions: tuple[Decision, ...],
    reviews: list[dict[str, object]] | None = None,
    *,
    bypass_verdicts: bool = False,
) -> str:
    held = Counter(d.caused_by for d in decisions if d.caused_by is not None)
    jobs = []
    for d in decisions:
        item = {
            "id": d.job.id,
            "operation": d.job.operation,
            "status": d.status.value,
            "reason": decided(d),
            "code": d.cause.why.value,
            "waits_for": list(d.waits_for),
            "caused_by": d.caused_by,
            "holds_up": held[d.job.id],
        }
        if d.status is Status.PLACE and d.placement is not None:
            item["moves"] = [{"from": old, "to": new} for old, new in d.placement.moves]
        jobs.append(item)
    plan = {"left_out": list(dag.left_out), "reviews": reviews or [], "jobs": jobs}
    if bypass_verdicts:
        plan["bypass_verdicts"] = True
    if dag.gated:
        plan["gated"] = [
            {"id": job.id, "status": job.status.name.lower(), "outputs": list(job.outputs)} for job in dag.gated
        ]
    return json.dumps(plan, indent=2)
