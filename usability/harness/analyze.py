#!/usr/bin/env python3
"""Grade every trial, summarise its logged `spit` calls, and audit its transcript.

usage: analyze.py <runs.json> [run ...]

runs.json maps each run to its scenario and, optionally, where its plan and
transcript are:

    {"s1-logs-a": {"scenario": "s1-logs", "model": "larger",
                   "transcript": "/path/to/agent.jsonl"},
     "s2-followup-a": {"scenario": "s2-cohort", "key": "key-followup",
                       "sandbox": "s2-cohort-a", "model": "larger"}}

`key` picks the answer key folder (default `key`), `sandbox` the trial folder
when it differs from the run name, and `plan` the plan file (default
`plan.spitdag` in the sandbox). The transcript is a Claude Code JSONL
transcript; the audit counts its tool calls and flags any that touch this
repository, the private folder, another trial's sandbox, or the web.

Writes <SPIT_PRIVATE>/results/<run>.json and prints a table.
"""
import json
import os
import re
import sys
import tempfile
from collections import Counter

HARNESS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HARNESS))
TRIALS = os.environ.get("SPIT_TRIALS", "/tmp/spit-trials")
PRIVATE = os.environ.get("SPIT_PRIVATE", "/tmp/spit-trials-private")
sys.path.insert(0, HARNESS)
from archive import unpack  # noqa: E402
from grade import grade  # noqa: E402

FORBIDDEN = [REPO, PRIVATE]


def telemetry(run):
    d = os.path.join(PRIVATE, "logs", run)
    calls = []
    for n in sorted(os.listdir(d)) if os.path.isdir(d) else []:
        c = os.path.join(d, n)
        if not n[:1].isdigit():
            continue
        read = lambda f: open(os.path.join(c, f)).read() if os.path.exists(os.path.join(c, f)) else ""
        args = read("call.txt").split("args:", 1)[-1].strip()
        rc = read("exit").strip()
        calls.append({"n": int(n), "args": args, "rc": int(rc) if rc else None, "stderr": read("stderr")})
    sub = lambda a: (a.split() or ["?"])[0]
    diagnostics = [(c["n"], line) for c in calls for line in c["stderr"].splitlines()
                   if line.startswith(("error", "warning"))]
    kinds = Counter(re.sub(r"`[^`]*`", "`…`", re.sub(r"line \d+, column \d+: ", "", l)) for _, l in diagnostics)
    first_ok = next((c["n"] for c in calls if sub(c["args"]) in ("check", "dag") and c["rc"] == 0), None)
    return {
        "spit_calls": len(calls),
        "by_command": dict(Counter(sub(c["args"]) for c in calls)),
        "failed_calls": sum(1 for c in calls if c["rc"] not in (0, None)),
        "first_success_call": first_ok,
        "diagnostics": [f"#{n}: {l}" for n, l in diagnostics],
        "diagnostic_kinds": dict(kinds.most_common()),
    }


def audit(transcript, sandbox):
    if not transcript or not os.path.exists(transcript):
        return {"error": "no transcript"}
    others = [r for r in os.listdir(TRIALS) if r != sandbox] if os.path.isdir(TRIALS) else []
    tools, violations, tokens = Counter(), [], 0
    for line in open(transcript):
        try:
            msg = json.loads(line).get("message") or {}
        except json.JSONDecodeError:
            continue
        if not isinstance(msg, dict):
            continue
        tokens += (msg.get("usage") or {}).get("output_tokens", 0)
        for b in msg.get("content") if isinstance(msg.get("content"), list) else []:
            if b.get("type") != "tool_use":
                continue
            tools[b["name"]] += 1
            text = json.dumps(b.get("input", {}))
            hits = [f for f in FORBIDDEN if f in text]
            hits += [f"{TRIALS}/{r}" for r in others if re.search(re.escape(f"{TRIALS}/{r}") + r"(/|\"|\s|$)", text)]
            if b["name"] in ("WebSearch", "WebFetch"):
                hits.append("web")
            if hits:
                violations.append({"tool": b["name"], "hits": hits, "input": text[:300]})
    return {"tool_calls": dict(tools), "total_tool_calls": sum(tools.values()),
            "output_tokens": tokens, "violations": violations}


def main():
    runs = json.load(open(sys.argv[1]))
    names = sys.argv[2:] or list(runs)
    out = os.path.join(PRIVATE, "results")
    os.makedirs(out, exist_ok=True)
    # The answer keys are kept in scenarios.zip.
    keys = tempfile.TemporaryDirectory()
    unpack(os.path.join(HARNESS, "scenarios.zip"), keys.name)
    print(f'{"run":22} {"model":8} {"grade":5} {"jobs":10} {"spit":>4} {"fail":>4} {"1st ok":>6} {"tools":>5} {"viol":>4}')
    for run in names:
        meta = runs[run]
        sandbox = meta.get("sandbox", run)
        key = os.path.join(keys.name, "scenarios", meta["scenario"], meta.get("key", "key"), "expected.spitdag")
        plan = os.path.join(TRIALS, sandbox, meta.get("plan", "plan.spitdag"))
        result = {"run": run, "model": meta.get("model"), "grade": grade(key, plan),
                  "telemetry": telemetry(meta.get("logs", sandbox)),
                  "audit": audit(meta.get("transcript"), sandbox)}
        json.dump(result, open(os.path.join(out, run + ".json"), "w"), indent=2)
        g, t, a = result["grade"], result["telemetry"], result["audit"]
        jobs = f'{g.get("matched_jobs", 0)}/{g.get("expected_jobs", "?")}+{len(g.get("extra", []))}'
        print(f'{run:22} {str(meta.get("model")):8} {"PASS" if g["pass"] else "FAIL":5} {jobs:10} '
              f'{t["spit_calls"]:>4} {t["failed_calls"]:>4} {str(t["first_success_call"]):>6} '
              f'{str(a.get("total_tool_calls", "-")):>5} {len(a.get("violations", [])):>4}')


if __name__ == "__main__":
    main()
