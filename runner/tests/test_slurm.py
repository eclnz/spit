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
from spit_bash.slurm.executor import job_name
from support import EXCLAIM, UPPER, document, job_entry, placed, sample
from fakeslurm.cleanup import shutdown

FAKE = Path(__file__).parent / "fakeslurm"
FAIL = "import sys; sys.exit(3)"
# Slow enough for the fake to stop it at its time limit.
SLOW = "import sys, time; from pathlib import Path; time.sleep(2); Path(sys.argv[2]).write_text('done')"
COPY = "from pathlib import Path; import sys; Path(sys.argv[2]).write_text(Path(sys.argv[1]).read_text())"
FAST = """
status_interval = 0.5
max_status_interval = 1
result_interval = 0.2
visibility = 2
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
        shutdown(self.fake)
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

    def killed_while_queued(self, data):
        """Start a run of `data` whose tasks stay queued, and kill its controller as a login node's limits would."""
        process = self.start("run", str(self.dag(data)), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.tasks())
        process.kill()
        process.communicate()

    def submissions(self):
        log = self.fake / "submissions.log"
        return [json.loads(line)["args"] for line in log.read_text().splitlines()] if log.exists() else []

    def option(self, name):
        """The value of option `name` of each sbatch call, in order."""
        return [next(arg for arg in args if arg.startswith(f"--{name}=")) for args in self.submissions()]

    def tasks(self):
        return [json.loads(path.read_text()) for path in sorted((self.fake / "tasks").glob("*.json"))]

    def wait_for(self, condition, seconds=30):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if condition():
                return
            time.sleep(0.1)
        self.fail("timed out waiting")


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
        for option in ("--array=0-0", "--cpus-per-task=1", "--mem=1024M", "--time=60", "--no-requeue",
                       "--account=lab", "--partition=short", "--qos=low"):
            self.assertIn(option, sent[0])
        self.assertFalse(any(arg.startswith("--signal") for arg in sent[0]))
        # The folder's name alone, whatever the pipeline, and the intent in the comment and the script.
        self.assertRegex(self.option("job-name")[0], r"^--job-name=spit-[0-9a-f]{8}$")
        intent = self.option("comment")[0].removeprefix("--comment=spit:")
        self.assertEqual(Path(sent[0][-1]).name, f"{intent}.sh")
        script = json.loads((self.fake / "submissions.log").read_text().splitlines()[0])["script"]
        self.assertIn("export MARK=1\n", script)
        self.assertIn("all 2 jobs are current", self.spit_bash("plan", str(dag), executor="local").stdout)

    def test_a_run_counts_its_slurm_calls_in_call_logs_jsonl(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        run = json.loads((self.root / ".spit-bash" / "slurm" / "call_logs.jsonl").read_text().splitlines()[-1])
        queue_reads = len((self.fake / "squeue.log").read_text().splitlines())
        # The fake logs every squeue call, so each one the run made is in the call log under a reason.
        self.assertEqual(run["totals"]["squeue"], queue_reads)
        self.assertEqual(run["totals"]["sbatch"], len(self.submissions()))
        self.assertEqual(run["totals"]["squeue:start"], 1)

    def test_a_run_leaves_no_result_or_output_behind_and_squeue_asks_for_its_name(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(list((self.root / ".spit-bash" / "slurm" / "results").glob("*")), [])
        self.assertEqual(list((self.root / ".spit-bash" / "runs").glob("*/slurm/out/*")), [])
        name = job_name((self.root / ".spit-bash").resolve())
        calls = [json.loads(line) for line in (self.fake / "squeue.log").read_text().splitlines()]
        self.assertTrue(calls)
        for args in calls:
            self.assertIn(f"--name={name}", args)
            self.assertIn("all", args)

    def test_ready_jobs_of_one_operation_go_in_one_array(self):
        for name in "abcd":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("abcd", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, [f"{n}.txt" for n in "abcd"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.option("array"), ["--array=0-3"])
        self.assertEqual((self.root / "out" / "c.txt").read_text(), "C")

    def test_the_queue_cap_holds(self):
        self.profile(FAST + "jobs = 1\n")
        for name in "ab":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("ab", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, ["a.txt", "b.txt"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.option("array"), ["--array=0-0", "--array=0-0"])

    def test_submissions_keep_to_the_rate(self):
        self.profile(FAST + 'submit_rate = "1/2s"\n')
        (self.root / "other.txt").write_text("x")
        entries = [job_entry(1, ["source.txt"], "a.txt", UPPER), job_entry(2, ["other.txt"], "b.txt", UPPER)]
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, ["source.txt", "other.txt"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        times = [float(line) for line in (self.fake / "sbatch-times.log").read_text().splitlines()]
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

    def test_a_task_that_ends_without_a_result_fails_with_slurms_state_and_no_sacct(self):
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_VANISH": "1"})
        self.assertEqual(done.returncode, 1)
        self.assertIn("Slurm ended its task as NODE_FAIL", done.stderr)
        # The queue showed how it ended.
        self.assertFalse((self.fake / "sacct.log").exists())

    def test_a_job_killed_for_memory_is_retried_with_more(self):
        self.profile(FAST + 'retries = 1\nmemory = "1G"\n')
        done = self.spit_bash("run", str(self.dag(sample(self.root))), env={"FAKE_SLURM_OOM_UNDER": "1500"})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("out of memory", done.stdout)
        self.assertEqual(self.option("mem")[:2], ["--mem=1024M", "--mem=2048M"])
        self.assertTrue(list((self.root / ".spit-bash" / "runs").glob("*/jobs/work/upper.txt.attempt-2.log")))

    def test_a_job_at_its_time_limit_is_retried_with_more_time(self):
        self.profile(FAST + 'retries = 1\ntime = "1h"\n')
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.option("time")[:2], ["--time=60", "--time=120"])

    def test_without_retries_a_job_at_its_time_limit_fails_and_puts_back_its_output(self):
        (self.root / "slow.txt").write_text("earlier")
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 1)
        self.assertIn("as TIMEOUT", done.stderr)
        self.assertEqual((self.root / "slow.txt").read_text(), "earlier")

    def test_a_retry_stops_at_the_profiles_ceiling(self):
        self.profile(FAST + 'retries = 2\ntime = "1h"\nmax_time = "1h"\n')
        data = document(self.root, [job_entry(1, ["source.txt"], "slow.txt", SLOW)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(data)), env={"FAKE_SLURM_TIMEOUT_UNDER": "100"})
        self.assertEqual(done.returncode, 1)
        self.assertEqual(len(self.submissions()), 1)

    def test_retries_and_their_resources_last_across_runs(self):
        self.profile(FAST + 'retries = 1\nmemory = "1G"\n')
        dag = self.dag(sample(self.root))
        failed = self.spit_bash("run", str(dag), env={"FAKE_SLURM_OOM_UNDER": "5000"})
        self.assertEqual(failed.returncode, 1)
        again = self.spit_bash("run", str(dag), env={"FAKE_SLURM_OOM_UNDER": "5000"})
        self.assertEqual(again.returncode, 1)
        # The second run starts where the first stopped, at its one retry's memory, and retries no more.
        self.assertEqual(self.option("mem"), ["--mem=1024M", "--mem=2048M", "--mem=2048M"])

    def test_a_second_run_asks_for_a_time_from_the_first(self):
        self.profile(FAST + 'time_factor = 2\nmin_time = "10m"\n')
        dag = self.dag(sample(self.root))
        self.spit_bash("run", str(dag))
        self.spit_bash("run", str(dag), "--force")
        self.assertEqual(self.option("time"), ["--time=60", "--time=60", "--time=10", "--time=10"])

    def test_operations_named_local_run_on_the_controller(self):
        self.profile(FAST + 'local = ["op1"]\n')
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(self.submissions()), 1)

    def test_short_jobs_are_named_as_better_run_locally(self):
        for name in "abcde":
            (self.root / f"{name}.txt").write_text(name)
        entries = same_operation([job_entry(index, [f"{name}.txt"], f"out/{name}.txt", UPPER)
                                  for index, name in enumerate("abcde", 1)], "upper")
        done = self.spit_bash("run", str(self.dag(document(self.root, entries, [f"{n}.txt" for n in "abcde"]))))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("note: 5 jobs of upper took", done.stderr)

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
        self.assertIn("program not found: only-after-setup", self.spit_bash("plan", str(dag), executor="local").stdout)
        self.profile(FAST + f'setup = ["export PATH={tools}:$PATH"]\n')
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr + done.stdout)
        self.assertEqual((self.root / "copy.txt").read_text(), "hi")

    def test_an_error_writing_an_intent_fails_the_jobs_rather_than_hanging(self):
        patch = Path(self.directory.name) / "patch"
        patch.mkdir()
        (patch / "sitecustomize.py").write_text(
            "import errno\n"
            "from spit_bash.slurm import intent\n"
            "def full(*args):\n"
            "    raise OSError(errno.ENOSPC, 'No space left on device')\n"
            "intent.write = full\n")
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

    def test_slurm_and_local_runs_share_the_state(self):
        dag = self.dag(sample(self.root))
        self.spit_bash("run", str(dag))
        (self.root / "source.txt").write_text("again")
        local = self.spit_bash("run", str(dag), executor="local")
        self.assertEqual(local.returncode, 0, local.stderr)
        self.assertEqual((self.root / "final.txt").read_text(), "AGAIN!")
        self.assertIn("all 2 jobs are current", self.spit_bash("plan", str(dag)).stdout)


class RestartTests(SlurmCase):
    """A run decides from what jobs make and the work they do, and what the queue holds, as a local run does: not
    from operation names, from what an earlier controller remembered, or from what a cache shows."""

    def test_stopping_the_controller_cancels_its_tasks(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.tasks())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["CANCELLED"])
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertNotIn("queued or running", done.stderr)

    def test_a_new_controller_waits_on_queued_tasks_rather_than_submit_them_again(self):
        dag = sample(self.root)
        self.killed_while_queued(dag)
        (self.fake / "release").write_text("")
        done = self.spit_bash("run", str(self.dag(dag)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertRegex(done.stderr, "1 Slurm task of this folder is queued or running|recorded 1 job")
        self.assertEqual(len(self.submissions()), 2)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")

    def test_a_task_scancel_could_not_cancel_is_waited_on_by_the_next_run(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1", "FAKE_SLURM_SCANCEL_FAIL": "1"})
        self.wait_for(lambda: self.tasks())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["PENDING"])
        (self.fake / "release").write_text("")
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(self.submissions()), 2)

    def test_a_job_that_finished_while_no_controller_ran_is_recorded(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        self.killed_while_queued(data)
        (self.fake / "release").write_text("")
        self.wait_for(lambda: self.tasks()[0]["state"] == "COMPLETED")
        dag = self.dag(data)
        planned = self.spit_bash("plan", str(dag))
        self.assertIn("recorded 1 job that finished in Slurm while no run watched", planned.stderr)
        self.assertIn("1 job is current", planned.stdout)
        ran = self.spit_bash("run", str(dag), executor="local")
        self.assertEqual(ran.returncode, 0, ran.stderr)
        self.assertNotIn("start", ran.stdout)
        self.assertEqual(list((self.root / ".spit-bash" / "slurm" / "results").glob("*")), [])

    def test_plan_changes_nothing_while_a_run_is_going(self):
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.tasks())
        slurm = self.root / ".spit-bash" / "slurm"
        self.wait_for(lambda: any(json.loads(path.read_text())[0] is not None for path in slurm.glob("*.dispatch")))
        before = sorted(str(path) for path in slurm.rglob("*") if "controller" not in path.name)
        planned = self.spit_bash("plan", str(dag))
        self.assertIn("1 Slurm task of this folder is queued or running", planned.stderr)
        self.assertEqual(sorted(str(path) for path in slurm.rglob("*") if "controller" not in path.name), before)
        self.assertEqual([task["state"] for task in self.tasks()], ["PENDING"])
        (self.fake / "release").write_text("")
        out, err = process.communicate(timeout=60)
        self.assertEqual(process.returncode, 0, err)

    def test_a_local_run_refuses_while_slurm_tasks_are_queued(self):
        self.killed_while_queued(sample(self.root))
        done = self.spit_bash("run", str(self.dag(sample(self.root))), executor="local")
        self.assertEqual(done.returncode, 2)
        self.assertIn("still queued or running", done.stderr)

    def test_renaming_an_operation_waits_on_its_queued_task(self):
        data = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"])
        self.killed_while_queued(data)
        (self.fake / "release").write_text("")
        data["jobs"][0]["operation"] = "renamed"
        done = self.spit_bash("run", str(self.dag(data)))
        self.assertEqual(done.returncode, 0, done.stderr)
        # Its key and work are as they were, so its task is this job's, whatever the operation is called.
        self.assertEqual(len(self.submissions()), 1)
        self.assertEqual((self.root / "copy.txt").read_text(), "hi")

    def test_changing_a_jobs_work_cancels_its_queued_task_and_submits_it_once_that_has_left(self):
        self.killed_while_queued(document(self.root, [job_entry(1, ["source.txt"], "copy.txt", COPY)], ["source.txt"]))
        changed = document(self.root, [job_entry(1, ["source.txt"], "copy.txt", UPPER)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(changed)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("cancelled 1 Slurm task(s) of work this run does not do", done.stderr)
        self.assertEqual((self.root / "copy.txt").read_text(), "HI")
        self.assertEqual(self.tasks()[0]["state"], "CANCELLED")

    def test_removing_a_job_cancels_its_queued_task(self):
        self.killed_while_queued(sample(self.root))
        kept = document(self.root, [job_entry(3, ["source.txt"], "other.txt", COPY)], ["source.txt"])
        done = self.spit_bash("run", str(self.dag(kept)))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("cancelled 1 Slurm task(s) of work this run does not do", done.stderr)
        self.assertEqual(self.tasks()[0]["state"], "CANCELLED")

    def test_a_matching_job_name_does_not_authorize_cancellation(self):
        name = job_name((self.root / ".spit-bash").resolve())
        script = Path(self.directory.name) / "stray.sh"
        script.write_text("#!/bin/bash\nsleep 30\n")
        subprocess.run(["sbatch", "--parsable", f"--job-name={name}", str(script)],
                       env=dict(self.env, FAKE_SLURM_HOLD="1"), check=True, capture_output=True)
        other = subprocess.run(["sbatch", "--parsable", "--job-name=someone-else", str(script)],
                               env=dict(self.env, FAKE_SLURM_HOLD="1"), check=True, capture_output=True, text=True)
        done = self.spit_bash("run", str(self.dag(sample(self.root))))
        self.assertEqual(done.returncode, 0, done.stderr)
        states = {task["task"]: task["state"] for task in self.tasks()}
        self.assertEqual(states["1_0"], "PENDING")
        self.assertEqual(states[f"{other.stdout.strip()}_0"], "PENDING")

    def test_outputs_missing_just_after_a_slurm_run_are_looked_for_again(self):
        self.profile(FAST.replace("visibility = 2", 'visibility = "20s"'))
        dag = self.dag(sample(self.root))
        self.assertEqual(self.spit_bash("run", str(dag)).returncode, 0)
        # As a node's cache that still shows the output missing: it appears a moment into the plan.
        final = self.root / "final.txt"
        hidden = self.root / ".final.txt.hidden"
        final.rename(hidden)
        threading.Timer(2.0, lambda: hidden.rename(final)).start()
        self.assertIn("all 2 jobs are current", self.spit_bash("plan", str(dag)).stdout)

    def test_an_output_deleted_after_a_slurm_run_waits_at_most_its_bound(self):
        self.profile(FAST.replace("visibility = 2", 'visibility = "4s"'))
        dag = self.dag(sample(self.root))
        self.assertEqual(self.spit_bash("run", str(dag)).returncode, 0)
        (self.root / "final.txt").unlink()
        started = time.monotonic()
        planned = self.spit_bash("plan", str(dag))
        self.assertLess(time.monotonic() - started, 15)
        self.assertIn("1 job can start", planned.stdout)

    def test_a_second_controller_of_the_folder_is_refused(self):
        dag = self.dag(sample(self.root))
        lease = self.root / ".spit-bash" / "slurm" / "controller.json"
        lease.parent.mkdir(parents=True)
        lease.write_text(json.dumps({"host": "login2", "pid": 4242, "at": time.time()}))
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 2)
        self.assertIn("process 4242 on login2", done.stderr)
        self.assertEqual(self.submissions(), [])
        # One on this host whose process has gone does not hold the folder.
        lease.write_text(json.dumps({"host": socket.gethostname(), "pid": 2 ** 22 + 1, "at": time.time()}))
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertFalse(lease.exists())


class ProfileTests(unittest.TestCase):
    def test_a_profile_reads_its_keys_and_overrides(self):
        read = profiles.parse({"time": "2h", "memory": "4G", "submit_rate": "10/30s", "jobs": 5, "visibility": "90s",
                               "operations": {"big": {"memory": "64G", "partition": "bigmem"}},
                               "stages": {"pre": {"time": "10m"}}})
        self.assertEqual((read.resources.time, read.resources.memory), (7200.0, 4 << 30))
        self.assertEqual((read.submissions, read.per, read.jobs, read.visibility), (10, 30.0, 5, 90.0))
        self.assertEqual(read.operations["big"]["partition"], "bigmem")

    def test_a_wrong_key_or_value_is_named(self):
        for table, words in (({"tme": "1h"}, "unknown key 'tme'"), ({"time": "soon"}, "time must be a time"),
                             ({"operations": {"x": {"cores": 2}}}, "operations.x has unknown key 'cores'"),
                             ({"submit_rate": "fast"}, "submit_rate")):
            with self.assertRaises(DagError) as raised:
                profiles.parse(table)
            self.assertIn(words, str(raised.exception))

    def test_scheduler_limits_and_routing_cannot_be_overridden(self):
        for table in ({"submit_rate": "50/0s"}, {"status_interval": 0}, {"max_status_interval": 0}, {"result_interval": float("inf")},
                      *({"options": [option]} for option in
                        ("--requeue", "--arr=0-99", "--clusters=other", "-a0-99", "--wrap=echo"))):
            with self.subTest(table=table), self.assertRaises(DagError):
                profiles.parse(table)


if __name__ == "__main__":
    unittest.main()
