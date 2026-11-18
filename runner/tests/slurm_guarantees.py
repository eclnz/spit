"""Release checks for the runner's impact on Slurm when scheduler replies go missing.

Uses fake Slurm and injected command failures, so the checks submit nothing to a real cluster.

The counterexamples from slurm_state_model.py map to the uncertain-array test
(release-on-empty), the polling-rate test (unmetered-query), the held-probe
test (cancel-foreign), and the restart test (forget-on-crash). The epilog
test covers release-on-result.
"""

import asyncio
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch

from spit_bash.execute.options import Options
from spit_bash.model import Layout
from spit_bash.slurm import behaviour, commands, site
from spit_bash.slurm.checks import Check, Name, Status
from spit_bash.slurm.dispatch import Dispatches
from spit_bash.slurm.executor import Slurm
from spit_bash.slurm.profile import Profile
from spit_bash.slurm.reconcile import Task
from spit_bash.slurm.throttle import SubmitRate
from support import document, job_entry, new_run, read, sample
from test_slurm import COPY, FAST, SlurmCase
from fakeslurm.cleanup import shutdown

FAKE = Path(__file__).resolve().parent / "fakeslurm"


class SlurmGuarantees(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.addCleanup(shutdown, self.root / "fake")
        self.layout = Layout(self.root / "runner")
        dag = read(document(self.root, [], []))
        self.executor = Slurm(Profile(jobs=2), dag, Options(new_run(self.layout.runs), 1, None),
                              self.layout.folder / "slurm", {}, {}, {}, {}, (), {}, commands.Config())

    async def test_uncertain_array_remains_charged_to_the_submission_budget(self):
        arguments = ["--array=0-1", f"--job-name={self.executor.name}"]
        self.executor.folder.mkdir()
        for answer in (None, []):
            with self.subTest(queue_answer=answer):
                accepted = []

                async def submit(script, options):
                    self.assertEqual(options, arguments)
                    accepted.append(2)
                    if len(accepted) == 1:
                        raise commands.Unsure("Slurm accepted the array but the sbatch reply was lost")
                    return "22"

                with patch.object(commands, "submit", side_effect=submit), \
                        patch.object(commands, "snapshot", return_value=answer), \
                        patch.object(self.executor.throttle, "wait", new=AsyncMock()), \
                        patch("spit_bash.slurm.executor.SNAPSHOT_GAP", 0):
                    try:
                        await self.executor._submit(self.root / "array.sh", arguments, "first", 2)
                    except commands.Unsure:
                        pass
                self.assertEqual(sum(accepted), self.executor.profile.jobs,
                                 "a missing reply cannot free slots in the Slurm task budget")

    async def test_failed_queue_read_does_not_trigger_calls_at_the_result_poll_rate(self):
        self.executor.snapshot_at -= self.executor.status_interval + 1
        with patch.object(commands, "snapshot", return_value=None) as snapshot:
            for _ in range(3):
                if self.executor._snapshot_due():
                    await self.executor._observe()
            self.assertEqual(snapshot.await_count, 1,
                             "failed squeue calls must still count against the queue polling interval")

    def test_partial_array_retirement_persists_across_recovery(self):
        (self.root / "slurm").mkdir()
        dispatches = Dispatches(self.root / "slurm")
        dispatches.reserve("batch", 2)
        dispatches.accept("batch", "17")
        dispatches.reconcile((Task("17_0", "batch", 0, "COMPLETED"),))
        recovered = Dispatches(self.root / "slurm")
        self.assertEqual(set(recovered.tasks()), {"17_1"})
        recovered.reconcile(())
        self.assertEqual(set(recovered.tasks()), {"17_1"})

    async def test_a_restart_cannot_reset_the_submission_window(self):
        profile = Profile(submissions=1, per=1)
        (self.root / "slurm").mkdir()
        limiter = SubmitRate(self.root / "slurm", profile)
        limiter.charge()
        limiter.settled()
        recovered = SubmitRate(self.root / "slurm", profile)
        with self.assertRaises(asyncio.TimeoutError):
            await asyncio.wait_for(recovered.wait(), 0.05)

    async def test_accounting_terminal_evidence_identifies_the_submission(self):
        rows = "17_0|COMPLETED|spit:other\n17_1|CANCELLED by 2000|spit:expected\n"
        with patch.object(commands, "call", return_value=(0, rows, "")):
            states = await commands.accounting(["17_0", "17_1"], intents={"17_0": "expected", "17_1": "expected"})
        self.assertEqual(states, {"17_1": "CANCELLED"})

    async def test_cluster_qualified_ack_cannot_become_a_local_job_id(self):
        with patch.object(commands, "call", return_value=(0, "17;other\n", "")):
            with self.assertRaisesRegex(commands.Unsure, "17;other"):
                await commands.submit(self.root / "array.sh", ["--array=0-0"])

    async def test_held_queue_probe_leaves_another_invocations_jobs_alone(self):
        fake = self.root / "fake"
        fake.mkdir()
        probes = self.root / "probe"
        probes.mkdir()
        batch = self.root / "other-invocation.sh"
        batch.write_text("#!/bin/bash\nexit 0\n")
        other_name = "spit-site-queue"
        with patch.dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(fake)):
            other = await commands.submit(batch, ["--hold", f"--job-name={other_name}"])
            try:
                await behaviour.queue(self.executor.profile, probes, 64 * 1024**2, 60)
                rows = await commands.snapshot(other_name)
                self.assertIsNotNone(rows)
                self.assertIn((f"{other}_0", "PENDING"), {(row.task, row.state) for row in rows},
                              "a held array created by another invocation was cancelled")
            finally:
                commands.cancel([other])

    async def test_interruption_after_submit_cancels_the_probe_and_saves_a_report(self):
        passed = Check(Name.PROFILE, Status.PASS, "accepted")
        with patch.object(site, "_commands", return_value=[passed]), \
                patch.object(site, "_requests", return_value=passed), \
                patch.object(commands, "check_site_values", return_value={}), \
                patch.object(commands, "submit", return_value="17"), \
                patch.object(behaviour, "queue", return_value=passed), \
                patch.object(site, "_watch", side_effect=asyncio.CancelledError), \
                patch.object(commands, "cancel", return_value=[]) as cancel:
            try:
                await site.check(self.executor.profile, self.layout)
            except asyncio.CancelledError:
                pass
        with self.subTest(guarantee="cleanup"):
            cancel.assert_any_call(["17"])
        with self.subTest(guarantee="report"):
            report = site.load(self.layout)
            self.assertIsNotNone(report, "an interrupted check must save its report")
            self.assertTrue(report.failed)

    def test_restart_cannot_resubmit_an_array_whose_acceptance_was_unrecorded(self):
        fake = self.root / "fake"
        fake.mkdir()
        (self.root / "source.txt").write_text("hi")
        dag = self.root / "jobs.spitdag"
        dag.write_text(json.dumps(sample(self.root)))
        profile = self.root / "cluster.toml"
        profile.write_text('jobs = 1\nstatus_interval = "1s"\nresult_interval = 0.2\nvisibility = "5s"\n')
        run = ["run", str(dag), "--dir", str(self.layout.folder), "--executor", "slurm", "--profile", str(profile)]
        env = dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(fake))
        before_reply = """
import os
from spit_bash.cli import main
from spit_bash.slurm import commands
submit = commands.submit
async def interrupted(*args):
    await submit(*args)
    os._exit(137)
commands.submit = interrupted
raise SystemExit(main())
"""
        crashed = subprocess.run([sys.executable, "-c", before_reply, *run], env=dict(env, FAKE_SLURM_HOLD="1"),
                                 capture_output=True, text=True, timeout=20)
        self.assertEqual(crashed.returncode, 137, crashed.stderr)
        log = fake / "submissions.log"
        first = json.loads(log.read_text().strip())

        self.assertTrue(Path(first["args"][-1]).is_file(), "the intent must be durable before sbatch")
        hide_first = """
import sys
from spit_bash.cli import main
from spit_bash.slurm import commands
first = sys.argv.pop(1)
snapshot = commands.snapshot
async def without_first(*args):
    rows = await snapshot(*args)
    return None if rows is None else [row for row in rows if row.task.partition("_")[0] != first]
commands.snapshot = without_first
raise SystemExit(main())
"""
        recovered = subprocess.run([sys.executable, "-c", hide_first, str(first["id"]), *run], env=env,
                                   capture_output=True, text=True, timeout=20)
        self.assertEqual(len(log.read_text().splitlines()), 1,
                         "a restart spent another task slot before reconciling the first array")
        self.assertNotEqual(recovered.returncode, 0, recovered.stdout)

    def test_success_result_does_not_free_the_slot_while_slurm_is_completing(self):
        fake = self.root / "fake"
        fake.mkdir()
        (self.root / "source.txt").write_text("hi")
        dag = self.root / "jobs.spitdag"
        dag.write_text(json.dumps(sample(self.root)))
        profile = self.root / "cluster.toml"
        profile.write_text('jobs = 1\nstatus_interval = 0.5\nresult_interval = 0.2\n')
        run = ["run", str(dag), "--dir", str(self.layout.folder), "--executor", "slurm", "--profile", str(profile)]
        env = dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(fake),
                   FAKE_SLURM_EPILOG="1")
        extend_epilog = """
import sys
from pathlib import Path
from spit_bash.cli import main
from spit_bash.slurm import commands
folder = Path(sys.argv.pop(1))
snapshot = commands.snapshot
observations = 0
async def completing(*args):
    global observations
    rows = await snapshot(*args)
    if rows is not None and any(row.task == "1_0" and row.state == "COMPLETING" for row in rows):
        observations += 1
        if observations >= 3:
            (folder / "epilog").touch()
    return rows
commands.snapshot = completing
raise SystemExit(main())
"""
        child = subprocess.Popen([sys.executable, "-c", extend_epilog, str(fake), *run], env=env,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 45
            while not (fake / "epilog").exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(0.1)
            self.assertTrue((fake / "epilog").exists(), "the test must observe the allocation in COMPLETING")
            submissions = (fake / "submissions.log").read_text().splitlines()
            self.assertEqual(len(submissions), 1, "the second task was submitted before Slurm released the first")
        finally:
            (fake / "end-epilog").touch()
            try:
                _, stderr = child.communicate(timeout=45)
            except subprocess.TimeoutExpired:
                child.kill()
                child.communicate()
                raise
        self.assertEqual(child.returncode, 0, stderr)


class ChangedLimitGuarantees(SlurmCase):
    def test_recovery_does_not_submit_while_old_tasks_exceed_the_new_limit(self):
        self.profile(FAST + "jobs = 2\n")
        old_jobs = [job_entry(index, ["source.txt"], f"old-{index}.txt", COPY) for index in (1, 2)]
        for job in old_jobs:
            job["operation"] = "old"
        self.killed_while_queued(document(self.root, old_jobs, ["source.txt"]))
        self.wait_for(lambda: len(self.tasks()) == 2, seconds=10)
        self.assertEqual([row["state"] for row in self.tasks()], ["PENDING", "PENDING"])
        submitted = len(self.submissions())

        self.profile(FAST + "jobs = 1\n")
        replacement = document(self.root, [job_entry(1, ["source.txt"], "replacement.txt", COPY)], ["source.txt"])
        calls = self.fake / "calls.log"
        calls_before_recovery = len(calls.read_text().splitlines())
        child = self.start("run", str(self.dag(replacement)), env={"FAKE_SLURM_SCANCEL_FAIL": "1"})
        try:
            self.wait_for(lambda: "squeue" in calls.read_text().splitlines()[calls_before_recovery:], seconds=10)
            _, stderr = child.communicate(timeout=20)
            self.assertEqual(len(self.submissions()), submitted,
                             "the replacement added work above the new limit before Slurm cleared old tasks")
            self.assertEqual(child.returncode, 2, stderr)
            self.assertIn("could not request Slurm cancellation", stderr)
            logs = (self.root / ".spit-bash" / "runs").glob("*/run.log")
            self.assertTrue(any("could not request Slurm cancellation" in path.read_text() for path in logs))
        finally:
            if child.poll() is None:
                child.terminate()
                child.communicate(timeout=10)
            arrays = sorted({str(row["array"]) for row in self.tasks()})
            subprocess.run([str(FAKE / "scancel"), *arrays], env=self.env, timeout=5, check=True)


if __name__ == "__main__":
    unittest.main()
