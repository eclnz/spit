"""How a Slurm run treats the scheduler at scale: 2,001 jobs, 100 in each of 20 operations, against the fake Slurm.

Slow, so it runs only with SPIT_BASH_SCALE=1, as CI's runner-scale job sets. It uses the profile's default limits,
measures every call the run makes to Slurm and how many tasks were queued at once, and fails when a limit the
README promises does not hold. It prints what it measured.
"""

import json
import os
import resource
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from support import document, job_entry, placed

FAKE = Path(__file__).parent / "fakeslurm"
WIDTH = 100
DEPTH = 20
COPY = "from pathlib import Path; import sys; Path(sys.argv[2]).write_text(Path(sys.argv[1]).read_text())"
JOIN = "from pathlib import Path; import sys; Path(sys.argv[-1]).write_text(''.join(Path(p).read_text() for p in sys.argv[2:-1]))"
# Slow enough for the fake to stop it at a one-minute limit.
SLOW = "import sys, time; from pathlib import Path; time.sleep(2); Path(sys.argv[2]).write_text('done')"
# The profile's defaults, which a scale run keeps: at most 100 tasks queued, 50 sbatch calls a minute, squeue once a
# minute.
JOBS, SUBMISSIONS, PER, STATUS = 100, 50, 60.0, 60.0


def chain(root: Path) -> dict:
    """`WIDTH` sources, each through `DEPTH` steps of its own operation, then one job joining every last step."""
    entries = []
    number = 0
    for index in range(WIDTH):
        (root / f"in/{index}.txt").parent.mkdir(exist_ok=True)
        (root / f"in/{index}.txt").write_text(f"{index}\n")
    ids = {}
    for step in range(DEPTH):
        for index in range(WIDTH):
            number += 1
            ids[step, index] = number
            source = f"in/{index}.txt" if step == 0 else f"step{step - 1}/{index}.txt"
            entry = job_entry(number, [source], f"step{step}/{index}.txt", COPY,
                              depends_on=[] if step == 0 else [ids[step - 1, index]])
            entry["operation"] = f"step{step}"
            entries.append(entry)
    number += 1
    last = [f"step{DEPTH - 1}/{index}.txt" for index in range(WIDTH)]
    entry = job_entry(number, last, "joined.txt", JOIN, depends_on=[ids[DEPTH - 1, index] for index in range(WIDTH)])
    entry["operation"] = "join"
    entries.append(entry)
    return document(root, entries, [f"in/{index}.txt" for index in range(WIDTH)])


def logged(folder: Path, name: str) -> list:
    path = folder / name
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def most_queued(fake: Path) -> int:
    """The most tasks queued or running at once, from the fake's log of each task's states."""
    current, most = set(), 0
    for item in sorted(logged(fake, "events.log"), key=lambda item: item["at"]):
        if item["state"] in ("PENDING", "RUNNING"):
            current.add(item["task"])
        else:
            current.discard(item["task"])
        most = max(most, len(current))
    return most


def calls(fake: Path, command: str) -> int:
    path = fake / "calls.log"
    return sum(1 for line in path.read_text().splitlines() if line == command) if path.exists() else 0


@unittest.skipUnless(os.environ.get("SPIT_BASH_SCALE"), "set SPIT_BASH_SCALE=1 to run the scale test")
class ScaleTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name) / "data"
        self.root.mkdir()
        self.fake = Path(self.directory.name) / "slurm"
        self.fake.mkdir()
        self.env = dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(self.fake),
                        FAKE_SLURM_SLOTS="100")

    def tearDown(self):
        self.directory.cleanup()

    def run_dag(self, data: dict, profile: str, env: dict | None = None):
        dag = Path(self.directory.name) / "jobs.spitdag"
        dag.write_text(json.dumps(placed(data)))
        (Path(self.directory.name) / "cluster.toml").write_text(profile)
        started = time.monotonic()
        process = subprocess.Popen([sys.executable, "-m", "spit_bash", "run", str(dag), "--executor", "slurm",
                                    "--profile", str(Path(self.directory.name) / "cluster.toml")],
                                   env=dict(self.env, **(env or {})), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True)
        out, err = process.communicate(timeout=3600)
        usage = resource.getrusage(resource.RUSAGE_CHILDREN)
        return process.returncode, out, err, time.monotonic() - started, usage

    def report(self, name: str, seconds: float, usage) -> dict:
        sent = logged(self.fake, "submissions.log")
        sizes = [int(next(a for a in item["args"] if a.startswith("--array=")).split("=")[1].split("-")[1].split("%")[0]) + 1
                 for item in sent]
        # As the shell wrapper logged them, before the fake's Python started, whose start varies under load.
        stamps = self.fake / "sbatch-times.log"
        times = [float(line) for line in stamps.read_text().splitlines()] if stamps.exists() else []
        gaps = [later - earlier for earlier, later in zip(times, times[1:])]
        measured = {
            "seconds": round(seconds, 1), "tasks": sum(sizes), "sbatch": len(sent),
            "largest array": max(sizes, default=0), "smallest gap between sbatch calls": round(min(gaps, default=0), 2),
            "most sbatch calls in any minute": max((sum(1 for t in times if start <= t < start + 60) for start in times),
                                                   default=0),
            "squeue": calls(self.fake, "squeue"), "sacct": calls(self.fake, "sacct"),
            "scancel": calls(self.fake, "scancel"), "scontrol": calls(self.fake, "scontrol"),
            "most tasks queued at once": most_queued(self.fake),
            "controller cpu seconds": round(usage.ru_utime + usage.ru_stime, 1),
        }
        print(f"\n{name}: " + json.dumps(measured, indent=1), file=sys.stderr)
        return measured

    def test_two_thousand_jobs_keep_within_the_profiles_limits(self):
        code, out, err, seconds, usage = self.run_dag(chain(self.root), "")
        self.assertEqual(code, 0, err[-3000:])
        self.assertIn(f"{WIDTH * DEPTH + 1} done", err)
        measured = self.report("2,001 jobs", seconds, usage)
        self.assertEqual(measured["tasks"], WIDTH * DEPTH + 1)
        self.assertLessEqual(measured["most tasks queued at once"], JOBS)
        self.assertLessEqual(measured["most sbatch calls in any minute"], SUBMISSIONS)
        self.assertGreaterEqual(measured["smallest gap between sbatch calls"], 0.95 * PER / SUBMISSIONS)
        # One squeue a status interval, and one as the run starts.
        self.assertLessEqual(measured["squeue"], seconds / STATUS + 2)
        self.assertEqual((measured["sacct"], measured["scancel"]), (0, 0))
        self.assertEqual(measured["scontrol"], 1)
        files = self.root / ".spit-bash" / "runs"
        self.assertEqual(list(files.glob("*/slurm/results/*")), [])
        self.assertEqual(list(files.glob("*/slurm/batches/*")), [])
        self.assertEqual(list(files.glob("*/slurm/out/*")), [])

    def test_a_hundred_jobs_stopped_at_their_limit_together_cost_few_sacct_calls(self):
        entries = []
        for index in range(WIDTH):
            (self.root / f"{index}.txt").write_text("x")
            entry = job_entry(index + 1, [f"{index}.txt"], f"slow/{index}.txt", SLOW)
            entry["operation"] = "slow"
            entries.append(entry)
        data = document(self.root, entries, [f"{index}.txt" for index in range(WIDTH)])
        code, out, err, seconds, usage = self.run_dag(data, 'retries = 1\ntime = "1m"\n',
                                                      {"FAKE_SLURM_TIMEOUT_UNDER": "2"})
        self.assertEqual(code, 0, err[-3000:])
        self.assertIn(f"{WIDTH} done", err)
        measured = self.report("100 jobs at their time limit", seconds, usage)
        # Most are stopped and retried; a task stopped while Python still starts loses the signal, and finishes.
        self.assertGreater(out.count("retry"), WIDTH // 2)
        # Each sacct call covers the tasks that ended within two seconds of each other.
        self.assertLessEqual(measured["sacct"], 10)
        self.assertLessEqual(measured["most tasks queued at once"], JOBS)


if __name__ == "__main__":
    unittest.main()
