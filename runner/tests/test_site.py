"""`spit-bash check-site`: what it finds on a fake Slurm, how it judges what the probe's tasks recorded, and how a run
reads the record it leaves."""

import dataclasses
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from spit_bash.model import DagError, Layout
from spit_bash.slurm import profile as profiles
from spit_bash.slurm import behaviour, probe, site
from spit_bash.slurm.checks import Check, Name, Status
from spit_bash.slurm.judge import Seen, judge
from spit_bash.slurm.probe import Claim
from spit_bash.slurm.site import Report
from support import placed, sample

FAKE = Path(__file__).parent / "fakeslurm"
HERE = probe.Identity("hpc", 2000, "1.0", 16, "login")
PROFILE = profiles.Profile(visibility=60.0)


def attempt(outcome):
    return probe.Attempt(outcome)


def found(host="c1", index=0, by_controller=Claim.EXISTS, by_node=Claim.CREATED, contested=None, quiet=True,
          why="", accounting=None, **identity):
    """What a healthy node's task writes, with whatever a test spoils."""
    contested = contested if contested is not None else (Claim.CREATED if index == 0 else Claim.EXISTS)
    return probe.Found(
        index=index, identity=probe.Identity(**{**dataclasses.asdict(HERE), "host": host, **identity}), umask=18,
        python="3.12.3", job="7", by_controller=probe.Attempt(by_controller, why),
        by_node=probe.Attempt(by_node, why), contested=probe.Attempt(contested),
        accounting=accounting or probe.Accounting(True), quiet=quiet, written=100.0)


def seen(host="c1", index=0, up_lag=2.0, down_lag=3.0, blocked=Claim.EXISTS, **changes):
    return Seen(found=found(host, index, **changes), found_at=100.0 + up_lag,
                heard=probe.Heard(index, True, 200.0 + down_lag, 200.0), blocked=attempt(blocked))


def by_name(checks):
    return {check.name: check for check in checks}


class JudgeTests(unittest.TestCase):
    """`judge` is pure, so each way a cluster can be unsafe is told here from records, without one."""

    def judge(self, found, profile=PROFILE, login=True):
        return by_name(judge(profile, found, HERE, login))

    def test_a_healthy_cluster_passes_every_check(self):
        checks = self.judge([seen("c1", 0), seen("c2", 1)])
        self.assertEqual({check.status for check in checks.values()}, {Status.PASS})
        self.assertEqual(set(checks), {Name.PROBE, Name.TOOLS, Name.EXCLUSIVE, Name.VISIBILITY, Name.ACCOUNTING})

    def test_an_exclusive_create_that_succeeds_over_the_controllers_file_fails(self):
        # A node that made a file the controller already made: the server did not decide.
        checks = self.judge([seen("c1", 0, by_controller=Claim.CREATED)])
        self.assertIs(checks[Name.EXCLUSIVE].status, Status.FAIL)
        self.assertIn("c1", checks[Name.EXCLUSIVE].detail)

    def test_an_exclusive_create_by_the_controller_over_a_nodes_file_fails(self):
        checks = self.judge([seen("c1", 0, blocked=Claim.CREATED)])
        self.assertIs(checks[Name.EXCLUSIVE].status, Status.FAIL)

    def test_two_nodes_that_both_win_a_race_fail(self):
        checks = self.judge([seen("c1", 0), seen("c2", 1, contested=Claim.CREATED)])
        self.assertIs(checks[Name.EXCLUSIVE].status, Status.FAIL)
        self.assertIn("2 nodes", checks[Name.EXCLUSIVE].detail)

    def test_an_exclusive_create_that_errors_fails(self):
        checks = self.judge([seen("c1", 0, by_node=Claim.FAILED, why="Operation not supported")])
        self.assertIs(checks[Name.EXCLUSIVE].status, Status.FAIL)

    def test_a_lag_over_the_bound_fails_and_says_what_to_set(self):
        checks = self.judge([seen("c1", 0, down_lag=75.0)])
        self.assertIs(checks[Name.VISIBILITY].status, Status.FAIL)
        self.assertIn("at least 151s", checks[Name.VISIBILITY].detail)

    def test_a_lag_over_half_the_bound_warns(self):
        checks = self.judge([seen("c1", 0, up_lag=40.0)])
        self.assertIs(checks[Name.VISIBILITY].status, Status.WARN)

    def test_a_lag_measured_by_a_node_that_saw_the_ping_at_once_does_not_count(self):
        # The node's first look found the ping already there, so its cache never held its absence.
        found = seen("c1", 0, up_lag=1.0, down_lag=50.0, quiet=False)
        self.assertIs(self.judge([found])["visibility"].status, Status.PASS)

    def test_a_file_seen_before_it_was_written_warns_of_clocks(self):
        checks = self.judge([seen("c1", 0, up_lag=-30.0)])
        self.assertIs(checks[Name.CLOCKS].status, Status.WARN)

    def test_a_node_that_runs_another_spit_bash_fails(self):
        checks = self.judge([seen("c1", 0, spit_bash="0.9")])
        self.assertIs(checks[Name.TOOLS].status, Status.FAIL)
        self.assertIn("c1 has spit-bash 0.9", checks[Name.TOOLS].detail)

    def test_a_node_that_runs_jobs_as_another_user_fails(self):
        checks = self.judge([seen("c1", 0, user="other", uid=3000)])
        self.assertIs(checks[Name.TOOLS].status, Status.FAIL)

    def test_a_node_that_cannot_ask_sacct_warns(self):
        checks = self.judge([seen("c1", 0, accounting=probe.Accounting(False, "no accounting"))])
        self.assertIs(checks[Name.ACCOUNTING].status, Status.WARN)
        self.assertIn("c1: no accounting", checks[Name.ACCOUNTING].detail)

    def test_a_controller_that_cannot_ask_sacct_warns(self):
        self.assertIs(self.judge([seen()], login=False)["accounting"].status, Status.WARN)

    def test_tasks_that_never_reported_fail(self):
        checks = self.judge([Seen(), Seen()])
        self.assertEqual(set(checks), {Name.PROBE})
        self.assertIs(checks[Name.PROBE].status, Status.FAIL)

    def test_one_task_that_did_not_start_warns(self):
        self.assertIs(self.judge([seen("c1", 0), Seen()])["probe"].status, Status.WARN)


class RecordTests(unittest.TestCase):
    """What a node writes is what the controller reads, and anything else reads as nothing."""

    def test_what_a_node_writes_is_what_the_controller_reads(self):
        original = found("c2", 1, by_node=Claim.FAILED, why="Operation not supported")
        self.assertEqual(probe.Found.from_json(json.loads(json.dumps(original.to_json()))), original)
        heard = probe.Heard(1, True, 205.5, 200.0)
        self.assertEqual(probe.Heard.from_json(json.loads(json.dumps(heard.to_json()))), heard)

    def test_a_file_that_is_not_what_a_node_writes_reads_as_none(self):
        self.assertIsNone(probe.Found.from_json({"index": 0}))
        self.assertIsNone(probe.Found.from_json({**found().to_json(), "by_node": {"outcome": "maybe"}}))
        self.assertIsNone(probe.Heard.from_json({"index": 0}))

    def test_a_report_round_trips_and_a_check_it_does_not_know_makes_it_unreadable(self):
        report = Report(1.0, "abc", "login", (Check(Name.QUEUE, Status.PASS, "listed"),), "/kept")
        self.assertEqual(Report.from_json(json.loads(json.dumps(report.to_json()))), report)
        data = report.to_json()
        data["checks"][0][0] = "something else"
        self.assertIsNone(Report.from_json(data))

    def test_every_file_a_probe_leaves_is_in_its_own_folder_and_named_once(self):
        paths = probe.Paths(Path("/probe"))
        names = [paths.by_controller, paths.by_node(0), paths.by_node(1), paths.contested, paths.found(0),
                 paths.found(1), paths.heard(0), paths.heard(1), paths.ping, paths.started, paths.received,
                 paths.states]
        self.assertEqual(len(set(names)), len(names))
        self.assertTrue(all(path.parent.parent == Path("/probe") for path in names))


class GateTests(unittest.TestCase):
    """What a run does with the record, for the profile it uses."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.layout = Layout(Path(self.directory.name) / ".spit-bash")
        self.addCleanup(self.directory.cleanup)

    def record(self, status, profile=PROFILE, at=None):
        site.save(self.layout, Report(time.time() if at is None else at, site.fingerprint(profile), "login",
                                      (Check(Name.VISIBILITY, status, "a file took 80s"),)))

    def test_no_record_is_a_note_and_a_run_goes_on(self):
        self.assertIn("no site check", site.gate(PROFILE, self.layout)[0])

    def test_a_profile_that_requires_a_check_refuses_without_one(self):
        with self.assertRaisesRegex(DagError, "requires a site check"):
            site.gate(profiles.Profile(require_site_check=True), self.layout)

    def test_a_failed_check_stops_a_run(self):
        self.record(Status.FAIL)
        with self.assertRaisesRegex(DagError, "visibility: a file took 80s"):
            site.gate(PROFILE, self.layout)

    def test_a_passed_check_lets_a_run_go_without_a_note(self):
        self.record(Status.PASS)
        self.assertEqual(site.gate(PROFILE, self.layout), [])

    def test_a_warning_does_not_stop_a_run(self):
        self.record(Status.WARN)
        self.assertEqual(site.gate(PROFILE, self.layout), [])

    def test_a_check_for_another_profile_is_no_check(self):
        self.record(Status.FAIL)
        other = profiles.Profile(visibility=60.0, resources=profiles.Resources(partition="long"))
        self.assertIn("no site check", site.gate(other, self.layout)[0])

    def test_an_old_check_is_a_note(self):
        self.record(Status.PASS, at=time.time() - 20 * 86400)
        self.assertIn("20 days old", site.gate(PROFILE, self.layout)[0])

    def test_a_record_that_cannot_be_read_is_no_record(self):
        (self.layout.folder / "site").mkdir(parents=True)
        (self.layout.folder / "site" / "site.json").write_text("{")
        self.assertIsNone(site.load(self.layout))


class CheckSiteTests(unittest.TestCase):
    """The command, against a fake Slurm that runs the probe's tasks on this machine."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.folder = Path(self.directory.name) / "runner"
        self.fake = Path(self.directory.name) / "slurm"
        self.fake.mkdir()
        self.env = dict(os.environ, PATH=f"{FAKE}{os.pathsep}{os.environ['PATH']}", FAKE_SLURM_DIR=str(self.fake))
        self.profile("visibility = 5\nstatus_interval = 5\n")

    def profile(self, text):
        self.profile_path = Path(self.directory.name) / "cluster.toml"
        self.profile_path.write_text(text)

    def check_site(self, *args, env=None):
        command = [sys.executable, "-m", "spit_bash", "check-site", "--dir", str(self.folder), "--profile",
                   str(self.profile_path), "--timeout", "60", *args]
        return subprocess.run(command, env=dict(self.env, **(env or {})), capture_output=True, text=True, timeout=120)

    def test_a_cluster_that_works_passes_and_leaves_a_record(self):
        done = self.check_site()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("site check: 10 passed, 0 warnings, 0 failed", done.stdout)
        record = site.load(Layout(self.folder))
        self.assertEqual(record.failed, ())
        # The probe's folder is removed once the checks pass.
        self.assertEqual([path.name for path in (self.folder / "site").iterdir()], ["site.json"])

    def test_the_check_makes_a_few_slurm_calls_and_none_in_a_loop(self):
        self.check_site()
        calls = (self.fake / "calls.log").read_text().split()
        # --test-only, the held array and the probe; the queue is read twice, around the held array's cancel.
        self.assertEqual({name: calls.count(name) for name in ("sbatch", "squeue", "scancel", "scontrol", "sacct")},
                         {"sbatch": 3, "squeue": 2, "scancel": 2, "scontrol": 1, "sacct": 3})

    def test_a_profile_slurm_refuses_fails_without_submitting_a_probe(self):
        done = self.check_site(env={"FAKE_SLURM_TEST_ONLY_FAIL": "1"})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("Slurm would not accept the profile's request", done.stdout)
        self.assertFalse((self.fake / "submissions.log").exists())

    def test_a_filesystem_slower_than_the_bound_fails(self):
        self.profile("visibility = 0.001\n")
        done = self.check_site()
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertRegex(done.stdout, r"fail  visibility .*visibility is 0s; set visibility to at least")
        # The probe's folder and its tasks' logs are kept to look at.
        self.assertIn("is kept at", done.stdout)

    def test_a_missing_sacct_warns_and_a_run_may_go_on(self):
        done = self.check_site(env={"FAKE_SLURM_SACCT_FAIL": "1"})
        self.assertEqual(done.returncode, 0, done.stdout)
        self.assertRegex(done.stdout, r"warn  accounting")

    def test_probe_tasks_that_never_start_fail_after_the_timeout(self):
        started = time.time()
        done = self.check_site("--timeout", "3", env={"FAKE_SLURM_HOLD": "1"})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("no probe task reported", done.stdout)
        self.assertLess(time.time() - started, 30)
        # Tasks still queued are cancelled, not left for the cluster to run later.
        self.assertIn("scancel", (self.fake / "calls.log").read_text())
        (self.fake / "release").write_text("")

    def test_dry_says_what_it_would_do_and_does_none_of_it(self):
        done = self.check_site("--dry")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("sbatch --array=0-2", done.stdout)
        self.assertIn("sbatch --hold --array=0-1", done.stdout)
        self.assertIn("probe", done.stdout)
        self.assertFalse((self.fake / "submissions.log").exists())
        self.assertFalse(self.folder.exists())

    def test_a_failed_check_stops_a_slurm_run_until_it_is_made_again(self):
        self.profile("visibility = 0.001\n")
        self.check_site()
        root = Path(self.directory.name) / "data"
        root.mkdir()
        (root / "source.txt").write_text("hi")
        dag = Path(self.directory.name) / "jobs.spitdag"
        dag.write_text(json.dumps(placed(sample(root))))
        run = subprocess.run([sys.executable, "-m", "spit_bash", "run", str(dag), "--dir", str(self.folder),
                              "--executor", "slurm", "--profile", str(self.profile_path)],
                             env=self.env, capture_output=True, text=True, timeout=120)
        self.assertEqual(run.returncode, 2, run.stdout)
        self.assertIn("the site check failed for this profile", run.stderr)
        # Only the check's own held array and probe were submitted: the refused run submitted nothing.
        self.assertEqual(len((self.fake / "submissions.log").read_text().splitlines()), 2)

    def test_a_scancel_that_cancels_nothing_fails_the_queue_check(self):
        done = self.check_site(env={"FAKE_SLURM_SCANCEL_NOOP": "1"})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertRegex(done.stdout, r"fail  queue .*`scancel --name` left .* in the queue")

    def test_a_squeue_that_hides_the_scripts_path_fails_the_queue_check(self):
        done = self.check_site(env={"FAKE_SLURM_BLANK_COMMAND": "1"})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertRegex(done.stdout, r"fail  queue .*command as `\(null\)`, not the script's path")

    def test_a_cancelled_task_that_never_hears_sigterm_fails(self):
        done = self.check_site(env={"FAKE_SLURM_NO_TERM": "1"})
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertRegex(done.stdout, r"fail  stop signal .*never received SIGTERM")

    def test_a_short_killwait_warns(self):
        done = self.check_site(env={"FAKE_SLURM_KILL_WAIT": "3"})
        self.assertEqual(done.returncode, 0, done.stdout)
        self.assertRegex(done.stdout, r"warn  stop signal .*kills it 3s later")

    def test_a_short_minjobage_says_how_often_a_run_will_read_the_queue(self):
        self.profile("visibility = 5\nstatus_interval = 60\n")
        done = self.check_site(env={"FAKE_SLURM_MIN_JOB_AGE": "20"})
        self.assertRegex(done.stdout, r"warn  limits .*reads the queue every 10s, not the profile's 60s")

    def test_a_scontrol_that_does_not_answer_warns_that_defaults_are_assumed(self):
        done = self.check_site(env={"FAKE_SLURM_SCONTROL_FAIL": "1"})
        self.assertRegex(done.stdout, r"warn  limits .*assumes MinJobAge 300s")

    def test_enforcement_ends_jobs_on_purpose_and_reads_what_slurm_called_them(self):
        done = self.check_site("--enforcement", env={"FAKE_SLURM_OOM_UNDER": "256", "FAKE_SLURM_TIMEOUT_UNDER": "2"})
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertRegex(done.stdout, r"pass  memory enforcement .*ended OUT_OF_MEMORY")
        self.assertRegex(done.stdout, r"pass  time limit .*ended TIMEOUT")

    def test_without_enforcement_no_job_is_ended_on_purpose(self):
        self.check_site()
        self.assertEqual(len((self.fake / "submissions.log").read_text().splitlines()), 2)


class BehaviourTests(unittest.TestCase):
    """What the checks of Slurm's own behaviour make of what they are told."""

    def test_limits_report_what_scontrol_says_and_the_interval_a_run_will_keep(self):
        found, kill = behaviour.limits(profiles.Profile(status_interval=60.0),
                                       {"MinJobAge": "300", "MaxArraySize": "1001", "KillWait": "30"})
        self.assertIs(found.status, Status.PASS)
        self.assertEqual(kill, 30.0)
        self.assertIn("reads the queue every 60s", found.detail)

    def test_a_minjobage_under_twice_the_interval_warns(self):
        found, _ = behaviour.limits(profiles.Profile(status_interval=60.0), {"MinJobAge": "60"})
        self.assertIs(found.status, Status.WARN)
        self.assertIn("every 30s, not the profile's 60s", found.detail)

    def test_sigterm_with_time_to_act_passes_and_too_little_time_warns(self):
        self.assertIs(behaviour.stop_signal(behaviour.Signal.RECEIVED, 30.0).status, Status.PASS)
        self.assertIs(behaviour.stop_signal(behaviour.Signal.RECEIVED, 2.0).status, Status.WARN)

    def test_a_signal_that_never_came_fails_and_one_never_tried_warns(self):
        self.assertIs(behaviour.stop_signal(behaviour.Signal.MISSING, 30.0).status, Status.FAIL)
        self.assertIs(behaviour.stop_signal(behaviour.Signal.UNTRIED, 30.0).status, Status.WARN)

    def test_a_job_ended_by_a_name_a_run_does_not_retry_on_warns(self):
        found = behaviour._enforced(behaviour.ENFORCED[0], "COMPLETED")
        self.assertIs(found.status, Status.WARN)
        self.assertIn("ended COMPLETED, not OUT_OF_MEMORY", found.detail)


if __name__ == "__main__":
    unittest.main()
