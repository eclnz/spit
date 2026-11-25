import json
import signal
import unittest

from support import document, job_entry, sample
from test_slurm import COPY, SlurmCase, same_operation


class ReleaseCase(SlurmCase):
    def setUp(self):
        super().setUp()
        self.runner = self.root / ".spit-bash"
        self.slurm = self.runner / "slurm"

    def two_copies(self):
        jobs = [job_entry(1, ["source.txt"], "a.txt", COPY), job_entry(2, ["source.txt"], "b.txt", COPY)]
        return document(self.root, same_operation(jobs, "copy"), ["source.txt"])

    def killed_once_accepted(self, data):
        process = self.start("run", str(self.dag(data)), env={"FAKE_SLURM_HOLD": "1"})
        self.wait_for(lambda: self.tasks() and self.accepted())
        process.kill()
        process.communicate()

    def accepted(self):
        return any(json.loads(path.read_text())[0] is not None for path in self.slurm.glob("*.dispatch"))

    def purge(self, *tasks):
        for path in (self.fake / "tasks").glob("*.json"):
            if not tasks or json.loads(path.read_text())["task"] in tasks:
                path.unlink()

    def dispatches(self):
        return sorted(path.name for path in self.slurm.glob("*.dispatch"))

    def release(self, *flags, env=None):
        return self.spit_bash("release", "--dir", str(self.runner), *flags, env=env)


class StoppedRunTests(ReleaseCase):
    def test_a_run_after_a_stop_submits_though_accounting_never_lists_tasks_cancelled_while_pending(self):
        env = {"FAKE_SLURM_UNSTARTED_UNACCOUNTED": "1", "FAKE_SLURM_MIN_JOB_AGE": "0"}
        dag = self.dag(sample(self.root))
        process = self.start("run", str(dag), env={"FAKE_SLURM_HOLD": "1", **env})
        self.wait_for(lambda: self.tasks())
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=30)
        self.assertEqual([task["state"] for task in self.tasks()], ["CANCELLED"])
        done = self.spit_bash("run", str(dag), env=env)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertNotIn("cannot verify", done.stderr)
        self.assertEqual(len(self.submissions()), 3)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")


class ReleaseTests(ReleaseCase):
    def test_a_run_blocked_by_a_reservation_it_cannot_verify_names_the_release_command(self):
        dag = self.dag(sample(self.root))
        self.killed_once_accepted(sample(self.root))
        self.purge()
        blocked = self.spit_bash("run", str(dag))
        self.assertNotEqual(blocked.returncode, 0)
        self.assertIn("accounting shows no end for it", blocked.stderr)
        self.assertIn("reservations kept and no new work submitted", blocked.stderr)
        self.assertIn(f"spit-bash release --dir {self.runner}", blocked.stderr)

    def test_release_lists_without_yes_and_changes_nothing(self):
        self.killed_once_accepted(sample(self.root))
        self.purge()
        before = self.dispatches()
        listed = self.release()
        self.assertEqual(listed.returncode, 0, listed.stderr)
        self.assertIn("the runner cannot verify 1 Slurm reservation", listed.stdout)
        self.assertIn("on your word", listed.stdout)
        self.assertIn("run again with --yes", listed.stdout)
        self.assertEqual(self.dispatches(), before)
        self.assertTrue(before)

    def test_releasing_a_reservation_deletes_its_batch_records_the_release_and_lets_a_run_submit(self):
        dag = self.dag(sample(self.root))
        self.killed_once_accepted(sample(self.root))
        self.purge()
        released = self.release("--yes")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertEqual(self.dispatches(), [])
        self.assertEqual(sorted((self.slurm / "intents").glob("*")), [])
        summaries = [json.loads(path.read_text()) for path in (self.runner / "runs").glob("*/summary.json")]
        self.assertIn({"ended": "released", "reservations": 1, "tasks": 1}, summaries)
        done = self.spit_bash("run", str(dag))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(self.submissions()), 3)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")

    def test_release_refuses_while_any_task_of_the_folder_is_queued(self):
        self.killed_once_accepted(self.two_copies())
        listed = sorted(task["task"] for task in self.tasks())
        self.purge(listed[0])
        before = self.dispatches()
        refused = self.release("--yes")
        self.assertEqual(refused.returncode, 1, refused.stdout)
        self.assertIn(f"scancel {listed[1]}", refused.stdout)
        self.assertEqual(self.dispatches(), before)

    def test_a_task_whose_accounting_did_not_answer_is_not_offered(self):
        self.killed_once_accepted(sample(self.root))
        self.purge()
        before = self.dispatches()
        refused = self.release("--yes", env={"FAKE_SLURM_SACCT_FAIL": "1"})
        self.assertEqual(refused.returncode, 1, refused.stdout)
        self.assertIn("run again once sacct answers", refused.stdout)
        self.assertEqual(self.dispatches(), before)

    def test_a_submission_whose_acceptance_was_never_seen_can_be_released(self):
        self.slurm.mkdir(parents=True)
        (self.slurm / "lost.dispatch").write_text("[null, [0]]")
        listed = self.release()
        self.assertIn("submission lost: the queue never showed whether Slurm accepted its submission", listed.stdout)
        released = self.release("--yes")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertEqual(self.dispatches(), [])

    def test_status_names_each_reservation_it_cannot_verify_and_the_command_that_clears_it(self):
        self.killed_once_accepted(sample(self.root))
        self.purge()
        shown = self.spit_bash("status", "--dir", str(self.runner))
        self.assertEqual(shown.returncode, 1, shown.stderr)
        self.assertIn("slurm: needs attention", shown.stdout)
        self.assertIn("accounting shows no end for it", shown.stdout)
        self.assertIn(f"spit-bash release --dir {self.runner}", shown.stdout)
        self.assertTrue(self.dispatches())

    def test_status_counts_queued_tasks_without_needing_attention(self):
        self.killed_once_accepted(sample(self.root))
        shown = self.spit_bash("status", "--dir", str(self.runner))
        self.assertIn("1 Slurm task of this folder is queued or running", shown.stdout)
        self.assertIn("slurm: ok", shown.stdout)

    def test_a_folder_with_no_reservations_makes_no_slurm_call(self):
        released = self.release("--yes")
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertIn("no Slurm reservations", released.stdout)
        shown = self.spit_bash("status", "--dir", str(self.runner))
        self.assertIn("no Slurm reservations", shown.stdout)
        self.assertFalse((self.fake / "squeue.log").exists())


if __name__ == "__main__":
    unittest.main()
