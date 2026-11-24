"""`spit-bash job`: one task of a batch, run as a Slurm job runs it, and the result file it writes."""

import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from spit_bash.execute import process
from spit_bash.execute.failure import CheckFailed, CommandFailed, InputNotVisible, OutputsMissing, Phase
from spit_bash.report.run_text import failure_words
from spit_bash.execute.options import Options
from spit_bash.files import Stamp
from spit_bash.model import Check, Side
from spit_bash.slurm import batch as batches
from spit_bash.slurm import claim
from spit_bash.slurm import result as results
from spit_bash.slurm.batch import Batch, Mode, Spec
from spit_bash.slurm.codec import decode, encode
from spit_bash.slurm.result import Outcome
from support import document, job_entry, new_run, read, sample

FAIL = "import sys; sys.exit(3)"
NOTHING = "pass"
SLOW = "import sys, time; from pathlib import Path; time.sleep(30); Path(sys.argv[2]).write_text('late')"


# The Slurm array a test's task belongs to, as Slurm tells a task in its environment: task 0 of it is `7_0`.
ARRAY = "7"


def task(dag, job_id, mode=Mode.RUN, expected=(), visibility=60.0, **resources):
    """A batch of one task for job `job_id`, written beside the run, and the path of its result; `resources` are the
    attempt, memory and time limit the task is submitted with."""
    run = new_run(dag.layout.runs)
    folder = dag.layout.folder / "slurm"
    spec = Spec(job_id, mode, run.folder / "jobs" / f"{job_id}.log", reason="output missing", expected=expected,
                **resources)
    jobs = [job for job in dag.jobs if job.id == job_id]
    path = folder / "intents" / f"i{job_id}.pickle"
    batches.write(Batch(batches.trim(dag, jobs, batches.by_folder(dag)), Options(run, enforce_memory=False),
                        (spec,), f"i{job_id}", folder, visibility), path)
    return path, results.path(folder, f"{ARRAY}_0")


def job_command(path):
    return [sys.executable, "-m", "spit_bash", "job", str(path), "0"]


ENVIRONMENT = dict(os.environ, SLURM_ARRAY_JOB_ID=ARRAY)


class JobTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        (self.root / "source.txt").write_text("hi")

    def tearDown(self):
        self.directory.cleanup()

    def test_a_job_that_succeeds_writes_its_output_and_a_result_with_its_hashes(self):
        dag = read(sample(self.root))
        path, result_path = task(dag, 1)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        self.assertEqual((self.root / "work" / "upper.txt").read_text(), "HI")
        result = results.read(result_path)
        self.assertEqual(result.outcome, Outcome.MADE)
        self.assertEqual((result.job, result.task, result.intent), (1, f"{ARRAY}_0", "i1"))
        self.assertEqual(result.fingerprint, dag.jobs[0].work.fingerprint)
        made = result.finished.made
        self.assertEqual(made.outputs["work/upper.txt"].size, 2)
        self.assertEqual(set(result.finished.output_hashes), {"out0"})
        self.assertEqual(set(result.finished.input_hashes), {"in0.0"})

    def test_a_result_carries_the_attempt_and_resources_its_task_was_submitted_with(self):
        dag = read(sample(self.root))
        path, result_path = task(dag, 1, attempt=2, memory=2 << 30, time_limit=120.0)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        result = results.read(result_path)
        self.assertEqual((result.attempt, result.memory, result.time_limit), (2, 2 << 30, 120.0))
        # A task submitted without them gives a result that reads without them.
        path, result_path = task(dag, 2)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        result = results.read(result_path)
        self.assertEqual((result.attempt, result.memory, result.time_limit), (None, None, None))

    def test_a_command_that_fails_gives_its_failure_and_status(self):
        dag = read(document(self.root, [job_entry(1, ["source.txt"], "out.txt", FAIL)], ["source.txt"]))
        path, result_path = task(dag, 1)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        result = results.read(result_path)
        self.assertEqual(result.outcome, Outcome.FAILED)
        self.assertEqual(result.failure, CommandFailed(Phase.COMMAND, 1, process.Exited(3)))

    def test_a_missing_output_fails_the_job_and_names_the_output(self):
        dag = read(document(self.root, [job_entry(1, ["source.txt"], "out.txt", NOTHING)], ["source.txt"]))
        path, result_path = task(dag, 1)
        subprocess.run(job_command(path), env=ENVIRONMENT)
        result = results.read(result_path)
        self.assertEqual(result.outcome, Outcome.FAILED)
        self.assertEqual(result.failure, OutputsMissing((("output", "out.txt"),), ()))

    def test_sigterm_puts_back_the_earlier_output_and_says_the_job_was_terminated(self):
        dag = read(document(self.root, [job_entry(1, ["source.txt"], "out.txt", SLOW)], ["source.txt"]))
        (self.root / "out.txt").write_text("earlier")
        path, result_path = task(dag, 1)
        started = subprocess.Popen(job_command(path), env=ENVIRONMENT)
        log = next(iter((dag.layout.runs).glob("*/jobs/1.log")), None)
        deadline = time.time() + 10
        while (log is None or "$ " not in log.read_text()) and time.time() < deadline:
            time.sleep(0.05)
            log = next(iter((dag.layout.runs).glob("*/jobs/1.log")), None)
        started.send_signal(signal.SIGTERM)
        self.assertEqual(started.wait(20), 0)
        self.assertEqual(results.read(result_path).outcome, Outcome.TERMINATED)
        self.assertEqual((self.root / "out.txt").read_text(), "earlier")

    def test_a_job_waits_until_its_node_sees_an_input_as_its_maker_recorded_it(self):
        dag = read(sample(self.root))
        made = self.root / "work" / "upper.txt"
        made.parent.mkdir()
        made.write_text("OLD")
        os.utime(made, ns=(10**18, 10**18))
        # As the other node wrote it: what the job that made it recorded.
        expected = (("work/upper.txt", (2, 2 * 10**18)),)
        path, result_path = task(dag, 2, expected=expected)
        started = subprocess.Popen(job_command(path), env=ENVIRONMENT)
        time.sleep(1.5)
        self.assertIsNone(results.read(result_path))
        made.write_text("HI")
        os.utime(made, ns=(2 * 10**18, 2 * 10**18))
        self.assertEqual(started.wait(30), 0)
        self.assertEqual(results.read(result_path).outcome, Outcome.MADE)
        self.assertEqual((self.root / "final.txt").read_text(), "HI!")

    def test_an_input_never_seen_as_recorded_fails_the_job_naming_it(self):
        dag = read(sample(self.root))
        made = self.root / "work" / "upper.txt"
        made.parent.mkdir()
        made.write_text("OLD")
        path, result_path = task(dag, 2, expected=(("work/upper.txt", (2, 5)),), visibility=1.0)
        subprocess.run(job_command(path), env=ENVIRONMENT)
        result = results.read(result_path)
        self.assertEqual(result.outcome, Outcome.FAILED)
        self.assertIsInstance(result.failure, InputNotVisible)
        self.assertEqual(result.failure.unseen[0][:2], ("work/upper.txt", (2, 5)))
        self.assertIn("did not show here as their jobs recorded them within 1s: work/upper.txt",
                      failure_words(dag.jobs[1], result.failure))
        self.assertFalse((self.root / "final.txt").exists())

    def test_a_task_whose_claim_another_holds_touches_nothing_and_says_busy(self):
        dag = read(sample(self.root))
        path, result_path = task(dag, 1)
        claims = dag.layout.folder / "slurm" / "claims"
        claims.mkdir(parents=True)
        (claims / claim.name("i1", 0)).write_text("3_0")
        subprocess.run(job_command(path), env=ENVIRONMENT)
        self.assertEqual(results.read(result_path).outcome, Outcome.BUSY)
        self.assertFalse((self.root / "work" / "upper.txt").exists())

    def test_a_task_runs_while_it_holds_its_claim(self):
        dag = read(sample(self.root))
        path, result_path = task(dag, 1)
        subprocess.run(job_command(path), env=ENVIRONMENT)
        self.assertEqual(results.read(result_path).outcome, Outcome.MADE)
        self.assertEqual((dag.layout.folder / "slurm" / "claims" / claim.name("i1", 0)).read_text(), f"{ARRAY}_0")

    def test_checks_alone_give_checked(self):
        dag = read(sample(self.root))
        path, result_path = task(dag, 1, Mode.CHECK)
        subprocess.run(job_command(path), env=ENVIRONMENT)
        self.assertEqual(results.read(result_path).outcome, Outcome.CHECKED)

    def test_a_task_that_cannot_read_its_batch_exits_2(self):
        missing = self.root / "nothing.pickle"
        done = subprocess.run(job_command(missing), capture_output=True, text=True)
        self.assertEqual(done.returncode, 2)
        self.assertIn("cannot read task 0", done.stderr)

    def test_no_result_is_none(self):
        self.assertIsNone(results.read(self.root / "none.json"))

    def test_a_trimmed_batch_holds_only_its_jobs_paths(self):
        dag = read(sample(self.root))
        trimmed = batches.trim(dag, [dag.jobs[1]], batches.by_folder(dag))
        self.assertEqual([job.id for job in trimmed.jobs], [2])
        self.assertEqual(set(trimmed.absolute), {"work/upper.txt", "final.txt", "source.txt"})


class CodecTests(unittest.TestCase):
    def test_failures_and_stamps_read_back_as_written(self):
        check = Check(Side.AFTER, "size", "output", "out.txt", ("test", "-s", "out.txt"))
        for value in (
            CommandFailed(Phase.VERIFY, 2, process.KilledSignal(1 << 20)),
            CheckFailed(check, process.KilledMemory(5, 4)),
            OutputsMissing((("output", "a.txt"),), ("a_real.txt",)),
            {"a.txt": Stamp(1, 2), "b": Stamp(3, 4, 5), "c": None},
            process.NotStarted("no such file"),
        ):
            self.assertEqual(decode(encode(value)), value)

    def test_an_unknown_type_is_refused(self):
        with self.assertRaises(ValueError):
            decode({"$": "os.system"})


if __name__ == "__main__":
    unittest.main()
