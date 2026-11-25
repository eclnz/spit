#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time
import uuid


PUBLISH = """
import json, os, pathlib, tempfile, time
def publish(path, data):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent)
    with os.fdopen(fd, "w") as stream:
        json.dump(dict(data, at=time.time()), stream)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
"""


class Experiment:
    def __init__(self) -> None:
        self.root = Path("/shared") / ("completion-events-" + uuid.uuid4().hex[:12])
        self.root.mkdir()
        self.calls: list[list[str]] = []
        self.submitted: list[str] = []
        self.waiting = False

    def call(self, *args: str, body: str | None = None) -> str:
        if self.waiting and args[0] in {"squeue", "sacct", "scontrol"}:
            raise AssertionError("waiting must use shared files only")
        self.calls.append(list(args))
        done = subprocess.run(args, input=body, text=True, capture_output=True, timeout=20)
        if done.returncode:
            raise RuntimeError(f"{shlex.join(args)}: {done.stderr}")
        return done.stdout.strip()

    def submit(self, code: str, *options: str, hold: bool = False) -> str:
        script = "#!/bin/bash\nexec python3 - <<'PY'\n" + PUBLISH + code + "\nPY\n"
        args = [
            "sbatch",
            "--parsable",
            "--no-requeue",
            "--cpus-per-task=1",
            "--mem=128M",
            "--time=00:01:00",
            f"--job-name={self.root.name}",
            "--output=/dev/null",
        ]
        if hold:
            args.append("--hold")
        id = self.call(*args, *options, body=script)
        if not id.isascii() or not id.isdigit():
            raise RuntimeError(f"unrecognised local job ID: {id!r}")
        self.submitted.append(id)
        return id

    def witness(self, parent: str, path: str, *, accounting: bool = False, hold: bool = False, tail: float = 0) -> str:
        code = f"""
import subprocess
record = {{"parent": {parent!r}, "witness": os.environ["SLURM_JOB_ID"],
          "terminated": True, "accounting": None, "accounting_calls": 0}}
"""
        if accounting:
            code += f"""
found = subprocess.run(["sacct", "--local", "-X", "-n", "-P", "-j", {parent!r},
                        "--format=JobID,State"], text=True, capture_output=True, timeout=10)
record.update(accounting=found.stdout, accounting_calls=1, accounting_exit=found.returncode)
"""
        code += f"publish({str(self.root / path)!r}, record)\ntime.sleep({tail!r})\n"
        return self.submit(code, f"--dependency=afterany:{parent}", hold=hold)

    def wait(self, paths: list[str], timeout: float = 210) -> dict[str, dict]:
        deadline = time.monotonic() + timeout
        found: dict[str, dict] = {}
        self.waiting = True
        try:
            while time.monotonic() < deadline:
                for name in paths:
                    if name in found:
                        continue
                    try:
                        found[name] = json.loads((self.root / name).read_text())
                    except FileNotFoundError:
                        pass
                if len(found) == len(paths):
                    return found
                time.sleep(2)
        finally:
            self.waiting = False
        missing = sorted(set(paths) - found.keys())
        raise RuntimeError(f"receipt deadline expired for {missing}; retained {self.root}")

    def run(self) -> dict:
        root = str(self.root)
        mixed = self.submit(
            f"""
index = int(os.environ["SLURM_ARRAY_TASK_ID"])
publish({root!r} + f"/mixed-start-{{index}}.json", {{"index": index}})
if index == 0:
    publish({root!r} + "/mixed-result.json", {{"success": True}})
elif index == 1:
    raise SystemExit(7)
elif index == 2:
    import signal
    os.kill(os.getpid(), signal.SIGKILL)
elif index == 3:
    memory = bytearray(512 * 1024 * 1024)
elif index == 4:
    time.sleep(150)
publish({root!r} + f"/mixed-end-{{index}}.json", {{"index": index}})
""",
            "--array=0-5%4",
            "--mem=64M",
            hold=True,
        )
        mixed_witness = self.witness(mixed, "mixed-witness.json", accounting=True)
        no_accounting = self.witness(mixed, "no-accounting.json")
        self.call("scancel", f"{mixed}_5")
        self.call("scontrol", "release", mixed)

        early = self.submit(
            f"""
publish({root!r} + "/early-result.json", {{"success": True}})
time.sleep(10)
publish({root!r} + "/early-end.json", {{"success": True}})
""",
            hold=True,
        )
        early_witness = self.witness(early, "early-witness.json", tail=20)
        helper_end = self.witness(early_witness, "helper-ended.json")
        self.call("scontrol", "release", early)

        invalid = self.witness("99999999", "invalid-dependency.json")

        abandoned = self.submit(
            f"""
publish({root!r} + "/abandoned-parent.json", {{"success": True}})
""",
            hold=True,
        )
        abandoned_witness = self.witness(abandoned, "abandoned-witness.json", hold=True)
        self.call("scancel", abandoned_witness)
        self.call("scontrol", "release", abandoned)

        barrier = self.submit(
            f"""
index = int(os.environ["SLURM_ARRAY_TASK_ID"])
time.sleep(2 if index == 0 else 24)
publish({root!r} + f"/barrier-{{index}}.json", {{"index": index}})
""",
            "--array=0-1",
            hold=True,
        )
        barrier_witness = self.witness(barrier, "barrier-witness.json")
        element_witness = self.witness(f"{barrier}_0", "element-witness.json")
        self.call("scontrol", "release", barrier)

        expected = [
            "mixed-witness.json",
            "no-accounting.json",
            "mixed-result.json",
            "early-result.json",
            "early-end.json",
            "early-witness.json",
            "helper-ended.json",
            "invalid-dependency.json",
            "abandoned-parent.json",
            "barrier-0.json",
            "barrier-1.json",
            "barrier-witness.json",
            "element-witness.json",
        ]
        print(
            f"Submitted {len(self.submitted)} arrays/jobs under {self.root}; waiting on files",
            file=sys.stderr,
            flush=True,
        )
        observed = self.wait(expected)
        assert observed["early-witness.json"]["at"] >= observed["early-end.json"]["at"]
        assert observed["early-end.json"]["at"] - observed["early-result.json"]["at"] >= 9
        assert observed["helper-ended.json"]["at"] - observed["early-witness.json"]["at"] >= 19
        assert observed["barrier-witness.json"]["at"] >= observed["barrier-1.json"]["at"]
        assert observed["element-witness.json"]["at"] >= observed["barrier-0.json"]["at"]
        assert observed["no-accounting.json"]["accounting_calls"] == 0
        assert not (self.root / "abandoned-witness.json").exists()
        assert not (self.root / "mixed-start-5.json").exists()
        accounting_calls = sum(row.get("accounting_calls", 0) for row in observed.values())
        before_cleanup = dict(Counter(call[0] for call in self.calls))
        rows = self.call(
            "sacct", "--local", "-X", "-n", "-P", "-j", ",".join(self.submitted), "--format=JobID,State,ExitCode"
        )
        live = self.call("squeue", "--local", "--me", f"--name={self.root.name}", "-h", "-r", "-t", "PD,R,CG")
        return {
            "slurm": self.call("sbatch", "--version"),
            "folder": root,
            "controller_calls_before_validation": before_cleanup,
            "witness_accounting_calls": accounting_calls,
            "controller_wait_queue_calls": 0,
            "ids": {
                "mixed": mixed,
                "mixed_witness": mixed_witness,
                "no_accounting": no_accounting,
                "early": early,
                "early_witness": early_witness,
                "helper_end": helper_end,
                "invalid": invalid,
                "abandoned": abandoned,
                "abandoned_witness": abandoned_witness,
                "barrier": barrier,
                "barrier_witness": barrier_witness,
                "element_witness": element_witness,
            },
            "observed": observed,
            "validation_accounting": rows,
            "live_after_validation": live,
            "calls": self.calls,
        }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="""Investigate dependency-triggered receipts on the local Docker Slurm cluster.

Run after `docker compose up -d`, from any directory:
    python3 runner/tests/cluster/completion_events.py --output /tmp/slurm-events.json

Submits isolated jobs as the cluster's hpc user. Waiting reads shared files only;
one final scoped queue read checks cleanup. This does not run or modify the executor.
"""
    )
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--output", type=Path, help="write the measured observations as JSON")
    args = parser.parse_args()
    if not args.inside:
        compose = Path(__file__).resolve().parent / "compose.yaml"
        result = subprocess.run(
            [
                "docker",
                "compose",
                "-f",
                str(compose),
                "exec",
                "-T",
                "-u",
                "hpc",
                "-w",
                "/shared",
                "login",
                "python3",
                "-",
                "--inside",
            ],
            input=Path(__file__).read_text(),
            text=True,
            stdout=subprocess.PIPE,
        )
        if result.returncode:
            raise SystemExit(result.returncode)
        report = json.loads(result.stdout)
        if args.output:
            args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({key: value for key, value in report.items() if key not in {"observed", "calls"}}, indent=2))
        return
    experiment = Experiment()
    try:
        report = experiment.run()
    except BaseException:
        if experiment.submitted:
            try:
                experiment.call("scancel", *experiment.submitted)
            except Exception as cleanup_error:
                print(f"cleanup request failed: {cleanup_error}", file=sys.stderr)
        raise
    print(json.dumps(report))


if __name__ == "__main__":
    main()
