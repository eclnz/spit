"""Runs with `--executor slurm` against a fake Slurm, tests/fakeslurm, which can fail as a real cluster can."""

import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import threading
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


class SlurmCase(unittest.TestCase):
    """A dataset, a fake Slurm and a fast profile, with helpers to run spit-bash against them."""
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


class SlurmTests(SlurmCase):
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
        # The folder's name alone, whatever the pipeline; the operation is in the comment.
        self.assertRegex(name, r"^--job-name=spit-[0-9a-f]{8}$")
        self.assertRegex(next(arg for arg in first if arg.startswith("--comment=")), r"^--comment=spit:[0-9a-f]{12}:op1$")
        scripts = [json.loads(line)["script"] for line in (self.fake / "submissions.log").read_text().splitlines()]
        self.assertIn("export MARK=1\n", scripts[0])
        self.assertEqual(self.ledger(), [])
        planned = self.spit_bash("plan", str(dag), executor="local")
        self.assertIn("all 2 jobs are current", planned.stdout)

    def test_a_run_leaves_no_batch_or_result_behind_and_squeue_asks_for_its_names(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        files = self.root / ".spit-bash" / "runs"
        self.assertEqual(list(files.glob("*/slurm/results/*")), [])
        self.assertEqual(list(files.glob("*/slurm/batches/*")), [])
        # Slurm's output of a task that succeeded is deleted with its result.
        self.assertEqual(list(files.glob("*/slurm/out/*.out")), [])
        calls = [json.loads(line) for line in (self.fake / "squeue.log").read_text().splitlines()]
        self.assertTrue(calls)
        for args in calls:
            named = next(arg for arg in args if arg.startswith("--name="))
            self.assertRegex(named, r"^--name=spit-[0-9a-f]{8}$")

    def test_many_tasks_at_their_time_limit_share_one_sacct_call(self):
        for name in "abcdef":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", SLOW)
                                  for index, name in enumerate("abcdef", 1)], "slow")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, [f"{n}.txt" for n in "abcdef"]))),
                              env={"FAKE_SLURM_TIMEOUT_UNDER": "100", "FAKE_SLURM_SLOTS": "6"})
        self.assertEqual(done.returncode, 1)
        self.assertEqual(done.stderr.count("as TIMEOUT"), 1, done.stderr)
        calls = (self.fake / "sacct.log").read_text().splitlines()
        self.assertLessEqual(len(calls), 2, calls)
        self.assertEqual(len(json.loads(calls[0])), 6)

    def test_a_retry_stops_at_the_profiles_ceiling(self):
        self.profile(FAST + 'retries = 2\ntime = "1h"\nmax_time = "1h"\n')
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 1)
        self.assertNotIn("retry", done.stdout)
        self.assertEqual(len(self.submissions()), 1)

    def test_short_jobs_are_named_as_better_run_locally(self):
        for name in "abcde":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("abcde", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, [f"{n}.txt" for n in "abcde"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("note: 5 jobs of upper took", done.stderr)
        self.assertIn("Name upper under `local`", done.stderr)

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

    def test_submissions_keep_to_the_rate(self):
        self.profile(FAST + 'submit_rate = "1/2s"\n')
        (self.root / "other.txt").write_text("x")
        entries = [job_entry(1, ["source.txt"], "a.txt", UPPER), job_entry(2, ["other.txt"], "b.txt", UPPER)]
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, ["source.txt", "other.txt"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        times = [json.loads(line)["at"] for line in (self.fake / "submissions.log").read_text().splitlines()]
        self.assertEqual(len(times), 2)
        self.assertGreaterEqual(times[1] - times[0], 1.9)

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
        comments = [next(a for a in args if a.startswith("--comment=")) for args in self.submissions()]
        self.assertEqual(len(comments), 1)
        self.assertTrue(comments[0].endswith(":op2"))

    def test_stopping_the_controller_cancels_its_tasks_and_the_next_run_settles_them(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["CANCELLED"])
        # Kept until a run sees how it ended: a cancelled task may still be stopping.
        self.assertEqual(len(self.ledger()), 1)
        started = time.monotonic()
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("ended with no result", done.stderr)
        # sacct said it was cancelled, so the run did not wait for a result that was not coming.
        self.assertLess(time.monotonic() - started, 30)
        self.assertEqual(self.ledger(), [])

    def test_a_task_scancel_could_not_cancel_stays_in_the_ledger(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1", "FAKE_SLURM_SCANCEL_FAIL": "1"})
        self.wait_for(lambda: self.ledger())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["PENDING"])
        self.assertEqual(len(self.ledger()), 1)
        (self.fake / "release").write_text("")
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        # Taken over while it runs, or read once it has finished: either way, not submitted again.
        self.assertRegex(done.stderr, "still queued or running|read 1 Slurm result")
        self.assertNotIn("cancelled", done.stderr)
        self.assertEqual(len(self.submissions()), 2)

    def test_a_task_of_this_folder_that_no_ledger_holds_is_cancelled(self):
        from spit_bash.slurm.executor import job_name
        name = job_name((self.root / '.spit-bash').resolve())
        script = Path(self.directory.name) / "stray.sh"
        script.write_text("#!/bin/bash\nsleep 30\n")
        subprocess.run(["sbatch", "--parsable", f"--job-name={name}", str(script)],
                       env=dict(self.env, FAKE_SLURM_HOLD="1"), check=True, capture_output=True)
        other = subprocess.run(["sbatch", "--parsable", "--job-name=someone-else", str(script)],
                               env=dict(self.env, FAKE_SLURM_HOLD="1"), check=True, capture_output=True, text=True)
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("cancelled 1 Slurm task", done.stderr)
        states = {task["task"]: task["state"] for task in self.tasks()}
        self.assertEqual(states["1_0"], "CANCELLED")
        self.assertEqual(states[f"{other.stdout.strip()}_0"], "PENDING")

    def test_an_error_writing_a_batch_fails_the_jobs_rather_than_hanging(self):
        patch = Path(self.directory.name) / "patch"
        patch.mkdir()
        (patch / "sitecustomize.py").write_text(
            "import errno\n"
            "from spit_bash.slurm import batch\n"
            "def full(*args):\n"
            "    raise OSError(errno.ENOSPC, 'No space left on device')\n"
            "batch.write = full\n")
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"PYTHONPATH": str(patch)})
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertIn("No space left on device", done.stderr)
        self.assertEqual(self.submissions(), [])

    def test_a_result_that_cannot_be_read_ends_its_job_rather_than_hanging(self):
        patch = Path(self.directory.name) / "patch"
        patch.mkdir()
        (patch / "sitecustomize.py").write_text(
            "import errno\n"
            "from spit_bash.slurm import executor\n"
            "def stale(*args):\n"
            "    raise OSError(errno.ESTALE, 'Stale file handle')\n"
            "executor.read = stale\n")
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"PYTHONPATH": str(patch)})
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertIn("no result appeared", done.stderr)

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


class DeterminismTests(SlurmCase):
    """A Slurm run decides from what jobs make and the work they do, as a local run does: not from operation names,
    timing, or what a cache shows."""

    def killed_while_queued(self, data):
        """Start a run of `data` whose tasks stay queued, and kill its controller as a login node's limits would."""
        process = self.start("run", str(self.dag(data)), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        process.kill()
        process.communicate()

    def test_renaming_an_operation_takes_over_its_queued_task(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        self.killed_while_queued(data)
        (self.fake / "release").write_text("")
        data["jobs"][0]["operation"] = "renamed"
        done = self.spit_bash("run", str(self.dag(data)))
        self.assertEqual(done.returncode, 0, done.stderr)
        # Its key and work are as they were, so its task is this job's, whatever the operation is called.
        self.assertRegex(done.stderr, "still queued or running|read 1 Slurm result")
        self.assertEqual(len(self.submissions()), 1)
        self.assertEqual((self.root / "copy.txt").read_text(), "hi")

    def test_removing_a_job_cancels_its_queued_task(self):
        data = sample(self.root)
        self.killed_while_queued(data)
        kept = document(self.root, [job_entry(3, ["source.txt"], "other.txt", COPY)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(kept)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("cancelled 1 Slurm task(s) an earlier run submitted for jobs this run does not run", done.stderr)
        self.assertEqual(self.tasks()[0]["state"], "CANCELLED")

    def test_a_task_sacct_calls_running_is_taken_over_though_squeue_misses_it(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        self.killed_while_queued(data)
        (self.fake / "release").write_text("")
        self.wait_for(lambda: self.tasks()[0]["state"] == "RUNNING")
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_HIDE_RUNNING": "1"})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertNotIn("ended with no result", done.stderr)
        self.assertEqual(len(self.submissions()), 1)

    def test_plan_leaves_a_running_controllers_ledger_alone(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.ledger())
        before = (self.root / ".spit-bash" / "slurm" / "ledger.jsonl").read_bytes()
        planned = self.spit_bash("plan", str(dag))
        self.assertIn("still queued or running", planned.stderr)
        self.assertEqual((self.root / ".spit-bash" / "slurm" / "ledger.jsonl").read_bytes(), before)
        (self.fake / "release").write_text("")
        out, err = process.communicate(timeout=60)
        self.assertEqual(process.returncode, 0, err)
        self.assertEqual(self.ledger(), [])

    def test_a_task_whose_result_the_state_holds_is_settled_without_waiting(self):
        self.profile(FAST.replace("result_wait = 2", "result_wait = 60"))
        dag = self.dag(sample(self.root))
        self.assertEqual(self.spit_bash("run", str(dag)).returncode, 0)
        # As a controller killed after recording a result, before writing that it had: the entry stays pending.
        state = [json.loads(line) for line in (self.root / ".spit-bash" / "state.jsonl").read_text().splitlines()]
        record = next(item for item in state if "record" in item and item["record"])
        ledger = self.root / ".spit-bash" / "slurm" / "ledger.jsonl"
        ledger.write_text(json.dumps({"token": "1-1-gone", "task": "999_0", "job": 1, "key": record["key"],
                                      "fingerprint": record["record"]["fingerprint"], "attempt": 1,
                                      "result": str(self.root / "gone.json"), "run": record["record"]["run"],
                                      "at": time.time()}) + "\n")
        started = time.monotonic()
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertLess(time.monotonic() - started, 30)
        self.assertNotIn("ended with no result", done.stderr)
        self.assertEqual(self.ledger(), [])

    def test_outputs_missing_just_after_a_slurm_run_are_looked_for_again(self):
        self.profile(FAST + 'cache_window = "20s"\n')
        dag = self.dag(sample(self.root))
        self.assertEqual(self.spit_bash("run", str(dag)).returncode, 0)
        # As a node's cache that still shows the output missing: it appears a moment into the plan.
        final = self.root / "final.txt"
        hidden = self.root / ".final.txt.hidden"
        final.rename(hidden)
        threading.Timer(2.0, lambda: hidden.rename(final)).start()
        planned = self.spit_bash("plan", str(dag), executor="local")
        self.assertIn("all 2 jobs are current", planned.stdout)

    def test_an_output_deleted_after_a_slurm_run_waits_at_most_the_window(self):
        self.profile(FAST + 'cache_window = "4s"\n')
        dag = self.dag(sample(self.root))
        self.assertEqual(self.spit_bash("run", str(dag)).returncode, 0)
        (self.root / "final.txt").unlink()
        started = time.monotonic()
        planned = self.spit_bash("plan", str(dag))
        self.assertLess(time.monotonic() - started, 15)
        self.assertIn("output missing", self.spit_bash("plan", str(dag), "--verbose").stdout)
        self.assertIn("1 job can start", planned.stdout)

    def test_a_second_controller_of_the_folder_is_refused(self):
        dag = self.dag(sample(self.root))
        claim = self.root / ".spit-bash" / "slurm" / "controller.json"
        claim.parent.mkdir(parents=True)
        claim.write_text(json.dumps({"host": "login2", "pid": 4242, "at": time.time()}))
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 2)
        self.assertIn("process 4242 on login2", done.stderr)
        self.assertEqual(self.submissions(), [])
        # One on this host whose process has gone does not hold the folder.
        claim.write_text(json.dumps({"host": socket.gethostname(), "pid": 2 ** 22 + 1, "at": time.time()}))
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertFalse(claim.exists())


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
