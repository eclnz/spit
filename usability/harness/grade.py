#!/usr/bin/env python3
"""Compare an agent's .spitdag with the expected one by the command lines its jobs run.

Product names, job ids and path rules the agent chose do not matter; only the
expanded argv of each job (and its verify commands) does. Paths are compared
relative to the dataset folder, so a leading `data/` is dropped.

usage: grade.py expected.spitdag actual.spitdag [--json]
"""
import json
import re
import sys
from collections import Counter


def norm_path(p):
    p = re.sub(r"^.*?/data/", "", p) if "/data/" in p else p
    return p[len("data/"):] if p.startswith("data/") else p


def render_arg(pieces):
    out = []
    for piece in pieces:
        if isinstance(piece, str):
            out.append(piece)
        elif isinstance(piece, dict) and "path" in piece:
            out.append(norm_path(piece["path"]))
        else:
            out.append(json.dumps(piece))
    return "".join(out)


def job_lines(dag):
    lines = Counter()
    for job in dag.get("jobs", []):
        cmd = " ".join(render_arg(a) for a in job.get("command", []))
        verifies = [" ".join(render_arg(a) for a in v) for v in job.get("verify", [])]
        line = cmd + "".join(f"  [verify: {v}]" for v in verifies)
        lines[line] += 1
    return lines


def grade(expected_path, actual_path):
    expected = job_lines(json.load(open(expected_path)))
    try:
        actual = job_lines(json.load(open(actual_path)))
    except FileNotFoundError:
        return {"pass": False, "error": f"no file at {actual_path}"}
    except json.JSONDecodeError as e:
        return {"pass": False, "error": f"not JSON: {e}"}
    missing = expected - actual
    extra = actual - expected
    by_tool = lambda c: dict(Counter(l.split()[0] for l in c.elements()))
    return {
        "pass": not missing and not extra,
        "expected_jobs": sum(expected.values()),
        "actual_jobs": sum(actual.values()),
        "matched_jobs": sum((expected & actual).values()),
        "expected_by_tool": by_tool(expected),
        "actual_by_tool": by_tool(actual),
        "missing": sorted(missing.elements()),
        "extra": sorted(extra.elements()),
    }


if __name__ == "__main__":
    result = grade(sys.argv[1], sys.argv[2])
    if "--json" in sys.argv:
        print(json.dumps(result, indent=2))
    else:
        print("PASS" if result["pass"] else "FAIL", {k: v for k, v in result.items() if k not in ("missing", "extra")})
        for k in ("missing", "extra"):
            for line in result.get(k, [])[:15]:
                print(f"  {k}: {line}")
            if len(result.get(k, [])) > 15:
                print(f"  ... {len(result[k]) - 15} more {k}")
