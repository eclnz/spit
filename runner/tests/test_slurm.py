"""Runs with `--executor slurm` against a fake Slurm, tests/fakeslurm, which can fail as a real cluster can."""

import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.slurm import profile as profiles
from support import EXCLAIM, UPPER, document, job_entry, placed, sample

FAKE = Path(__file__).parent / "fakeslurm"
FAIL = "import sys; sys.exit(3)"
# Slow enough for the fake to stop it at its time limit.
SLOW = "import sys, time; from pathlib import Path; time.sleep(2); Path(sys.argv[2]).write_text('done')"
COPY = "from pathlib import Path; import sys; Path(sys.argv[2]).write_text(Path(sys.argv[1]).read_text())"
FAST = """
gather = 0.2
status_interval = 0.5
result_interval = 0.2
result_wait = 2
"""


def same_operation(entries, operation):
    for entry in entries:
        entry["operation"] = operation
    return entries


class SlurmTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name) / "data"
        self.root.mkdir()
        self.fake = Path(self.directory.name) / "slurm"
        self.fake.mkdir()
        (self.root / "source.txt").write_text("hi")
        self.env = dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(self.fake))
        self.profile(FAST)

    def tearDown(self):
        # A held array's background process waits for its release.
        (self.fake / "release").write_text("")
        self.directory.cleanup()

    def profile(self, text):
        self.profile_path = Path(self.directory.name) / "cluster.toml"
        self.profile_path.write_text(text)

    def dag(self, data):
        path = Path(self.directory.name) / "jobs.spitdag"
        path.write_text(json.dumps(placed(data)))
        return path

    def spit_bash(self, *args, env=None, executor="slurm"):
        command = [sys.executable, "-m", "spit_bash", *args]
        if executor == "slurm" and args[0] in ("run", "plan"):
            command += ["--executor", "slurm", "--profile", str(self.profile_path)]
        return subprocess.run(command, env=dict(self.env, **(env or {})), capture_output=True, text=True, timeout=120)

    def start(self, *args, env=None):
        command = [sys.executable, "-m", "spit_bash", *args, "--executor", "slurm", "--profile", str(self.profile_path)]
        return subprocess.Popen(command, env=dict(self.env, **(env or {})), stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)

    def submissions(self):
        log = self.fake / "submissions.log"
        return [json.loads(line)["args"] for line in log.read_text().splitlines()] if log.exists() else []

    def tasks(self):
        return [json.loads(path.read_text()) for path in sorted((self.fake / "tasks").glob("*.json"))]

    def wait_for(self, condition, seconds=30):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if condition():
                return
            time.sleep(0.1)
        self.fail("timed out waiting")

    def ledger(self):
        path = self.root / ".spit-bash" / "slurm" / "ledger.jsonl"
        return path.read_text().splitlines() if path.exists() else []

    def test_a_dag_runs_through_slurm_and_is_current_after(self):
        self.profile(FAST + 'account = "lab"\npartition = "short"\nsetup = ["export MARK=1"]\noptions = ["--qos=low"]\n')
        dag = self.dag(sample(self.root))
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")
        self.assertIn("queued", done.stdout)
        sent = self.submissions()
        self.assertEqual(len(sent), 2)
        first = sent[0]
        for option in ("--array=0-0", "--cpus-per-task=1", "--mem=1024M", "--time=60", "--no-requeue",
                       "--signal=B:TERM@120", "--account=lab", "--partition=short", "--qos=low"):
            self.assertIn(option, first)
        name = next(arg for arg in first if arg.startswith("--job-name="))
        self.assertRegex(name, r"^--job-name=spit-[0-9a-f]{8}-op1$")
        self.assertIn("export MARK=1\n", Path(first[-1]).read_text())
        self.assertEqual(self.ledger(), [])
        planned = self.spit_bash("plan", str(dag), executor="local")
        self.assertIn("all 2 jobs are current", planned.stdout)

    def test_ready_jobs_of_one_operation_go_in_one_array(self):
        for name in "abcd":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("abcd", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, [f"{n}.txt" for n in "abcd"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        sent = self.submissions()
        self.assertEqual(len(sent), 1)
        self.assertIn("--array=0-3", sent[0])
        self.assertEqual((self.root / "out" / "c.txt").read_text(), "C")

    def test_the_queue_cap_and_array_limit_hold(self):
        self.profile(FAST + "jobs = 1\n")
        for name in "ab":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("ab", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, ["a.txt", "b.txt"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual([next(a for a in args if a.startswith("--array")) for args in self.submissions()],
                         ["--array=0-0", "--array=0-0"])

    def test_a_failed_job_fails_and_holds_back_its_dependents(self):
        data = document(self.root, [
            job_entry(1, ["source.txt"], "mid.txt", FAIL, dependents=[2]),
            job_entry(2, ["mid.txt"], "final.txt", EXCLAIM, depends_on=[1]),
        ], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)))
        self.assertEqual(done.returncode, 1)
        self.assertIn("command exited with status 3", done.stderr)
        self.assertEqual(len(self.submissions()), 1)

    def test_a_task_that_ends_without_a_result_fails_with_slurms_reason(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_VANISH": "op1"})
        self.assertEqual(done.returncode, 1)
        self.assertIn("Slurm ended its task as NODE_FAIL", done.stderr)
        self.assertEqual(self.ledger(), [])

    def test_without_sacct_the_run_goes_on_and_says_it_could_not_ask(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))),
                              env={"FAKE_SLURM_VANISH": "op1", "FAKE_SLURM_SACCT_FAIL": "1"})
        self.assertEqual(done.returncode, 1)
        self.assertIn("sacct could not say why", done.stderr)

    def test_a_job_killed_for_memory_is_retried_with_more(self):
        self.profile(FAST + 'retries = 1\nmemory = "1G"\n')
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_OOM_UNDER": "1500"})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("retry", done.stdout)
        self.assertIn("out of memory", done.stdout)
        memories = [next(a for a in args if a.startswith("--mem=")) for args in self.submissions()]
        self.assertEqual(memories[:2], ["--mem=1024M", "--mem=2048M"])
        logs = list((self.root / ".spit-bash" / "runs").glob("*/jobs/work/upper.txt.attempt-2.log"))
        self.assertTrue(logs)

    def test_a_job_at_its_time_limit_is_retried_with_more_time(self):
        self.profile(FAST + 'retries = 1\ntime = "1h"\n')
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 0, done.stderr)
        times = [next(a for a in args if a.startswith("--time=")) for args in self.submissions()]
        self.assertEqual(times[:2], ["--time=60", "--time=120"])

    def test_without_retries_a_job_at_its_time_limit_fails(self):
        (self.root / "slow.txt").write_text("earlier")
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 1)
        self.assertIn("as TIMEOUT", done.stderr)
        # The job put back the earlier output when Slurm stopped it.
        self.assertEqual((self.root / "slow.txt").read_text(), "earlier")

    def test_results_are_read_when_squeue_no_longer_lists_the_task(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_HIDE_RUNNING": "1"})
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_a_second_run_asks_for_a_time_from_the_first(self):
        self.profile(FAST + 'time_factor = 2\nmin_time = "10m"\n')
        dag = self.dag(sample(self.root))
        self.spit_bash("run", str(dag))
        self.spit_bash("run", str(dag), "--force")
        times = [next(a for a in args if a.startswith("--time=")) for args in self.submissions()]
        self.assertEqual(times, ["--time=60", "--time=60", "--time=10", "--time=10"])

    def test_operations_named_local_run_on_the_controller(self):
        self.profile(FAST + 'local = ["op1"]\n')
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        names = [next(a for a in args if a.startswith("--job-name=")) for args in self.submissions()]
        self.assertEqual(len(names), 1)
        self.assertTrue(names[0].endswith("-op2"))

    def test_stopping_the_controller_cancels_its_tasks(self):
        process = self.start("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.tasks())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["CANCELLED"])
        self.assertEqual(self.ledger(), [])

    def test_a_new_controller_takes_over_queued_tasks_instead_of_submitting_them_again(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.kill()
        process.communicate()
        (self.fake / "release").write_text("")
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("still queued or running", done.stderr)
        self.assertEqual(len(self.submissions()), 2)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")

    def test_results_left_while_no_controller_ran_are_recorded(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        dag = self.dag(data)
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.kill()
        process.communicate()
        (self.fake / "release").write_text("")
        self.wait_for(lambda: self.tasks() and self.tasks()[0]["state"] == "COMPLETED")
        planned = self.spit_bash("plan", str(dag), executor="local")
        self.assertIn("read 1 Slurm result", planned.stderr)
        self.assertIn("1 job is current", planned.stdout)
        ran = self.spit_bash("run", str(dag), executor="local")
        self.assertEqual(ran.returncode, 0, ran.stderr)
        self.assertNotIn("start", ran.stdout)
        self.assertEqual(self.ledger(), [])

    def test_a_local_run_refuses_while_slurm_tasks_are_queued(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.kill()
        process.communicate()
        done = self.spit_bash("run", str(dag), executor="local")
        self.assertEqual(done.returncode, 2)
        self.assertIn("still queued or running", done.stderr)

    def test_a_task_from_an_older_dag_is_cancelled_and_its_job_submitted_again(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        dag = self.dag(data)
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.kill()
        process.communicate()
        changed = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", UPPER)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(changed)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual((self.root / "copy.txt").read_text(), "HI")
        self.assertEqual(self.tasks()[0]["state"], "CANCELLED")

    def test_slurm_and_local_runs_share_the_state(self):
        dag = self.dag(sample(self.root))
        self.spit_bash("run", str(dag))
        (self.root / "source.txt").write_text("again")
        local = self.spit_bash("run", str(dag), executor="local")
        self.assertEqual(local.returncode, 0, local.stderr)
        self.assertEqual((self.root / "final.txt").read_text(), "AGAIN!")
        planned = self.spit_bash("plan", str(dag))
        self.assertIn("all 2 jobs are current", planned.stdout)

    def test_programs_the_setup_lines_add_are_found_before_planning(self):
        tools = Path(self.directory.name) / "tools"
        tools.mkdir()
        program = tools / "only-after-setup"
        program.write_text("#!/bin/sh\ncp \"$1\" \"$2\"\n")
        program.chmod(0o755)
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        data["jobs"][0]["commands"][0]["command"] = [["only-after-setup"], [{"path": "source.txt"}],
                                                     [{"path": "copy.txt"}]]
        data["executables"] = ["only-after-setup"]
        dag = self.dag(data)
        blocked = self.spit_bash("plan", str(dag), executor="local")
        self.assertIn("program not found: only-after-setup", blocked.stdout)
        self.profile(FAST + f'setup = ["export PATH={tools}:$PATH"]\n')
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual((self.root / "copy.txt").read_text(), "hi")


class ProfileTests(unittest.TestCase):
    def test_a_profile_reads_its_keys_and_overrides(self):
        read = profiles.parse({"time": "2h", "memory": "4G", "submit_rate": "10/30s", "jobs": 5,
                               "operations": {"big": {"memory": "64G", "partition": "bigmem"}},
                               "stages": {"pre": {"time": "10m"}}})
        self.assertEqual((read.resources.time, read.resources.memory), (7200.0, 4 << 30))
        self.assertEqual((read.submissions, read.per, read.jobs), (10, 30.0, 5))
        self.assertEqual(read.operations["big"]["partition"], "bigmem")

    def test_a_wrong_key_or_value_is_named(self):
        for table, words in (({"tme": "1h"}, "unknown key 'tme'"), ({"time": "soon"}, "time must be a time"),
                             ({"operations": {"x": {"cores": 2}}}, "operations.x has unknown key 'cores'"),
                             ({"submit_rate": "fast"}, "submit_rate")):
            with self.assertRaises(DagError) as raised:
                profiles.parse(table)
            self.assertIn(words, str(raised.exception))


if __name__ == "__main__":
    unittest.main()
