import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from spit_bash.model import DagError
from spit_bash.slurm.dispatch import Dispatches
from spit_bash.slurm.executor import Attempts
from spit_bash.slurm.profile import Profile
from spit_bash.slurm.throttle import SubmitRate
from support import read, sample


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_existing_reservation_arrays_keep_every_outstanding_index(self):
        (self.root / "a.dispatch").write_text('["17", [0, 2]]')
        (self.root / "b.dispatch").write_text("[null, [0, 1]]")
        records = Dispatches(self.root)
        self.assertEqual(set(records.tasks()), {"17_0", "17_2"})
        self.assertEqual(records.unknown, ("b",))
        records.accept("b", "18")
        self.assertEqual(json.loads((self.root / "b.dispatch").read_text()), ["18", [0, 1], []])

    def test_a_cancelled_index_is_kept_with_its_reservation(self):
        (self.root / "a.dispatch").write_text('["17", [0, 2, 3]]')
        records = Dispatches(self.root)
        records.cancel(["17_2", "17_0", "17_9", "18_0"])
        self.assertEqual(json.loads((self.root / "a.dispatch").read_text()), ["17", [0, 2, 3], [0, 2]])
        self.assertEqual(Dispatches(self.root).records["a"].cancelled, (0, 2))

    def test_invalid_cancelled_indices_are_refused(self):
        for text in ('["17",[0],[1,0]]', '["17",[0],[true]]', '["17",[0],[-1]]'):
            with self.subTest(text=text):
                (self.root / "a.dispatch").write_text(text)
                with self.assertRaises(DagError):
                    Dispatches(self.root)

    def test_invalid_reservations_cannot_become_available_capacity(self):
        path = self.root / "a.dispatch"
        for text in (
            "[17,[0]]",
            '["17;remote",[0]]',
            '["17\\n",[0]]',
            '["１７",[0]]',
            "[null,[]]",
            "[null,[true]]",
            "[null,[0.0]]",
            "[null,[-1]]",
            "[null,[0,0]]",
            "[null,[1,0]]",
            "[null,[0],1]",
            "{",
        ):
            with self.subTest(text=text):
                path.write_text(text)
                with self.assertRaises(DagError):
                    Dispatches(self.root)
                self.assertEqual(path.read_text(), text)

    def test_submission_history_keeps_its_five_field_format(self):
        limiter = SubmitRate(self.root, Profile())
        before = [limiter.host, limiter.clock_origin, limiter.period, [], True]
        limiter.path.write_text(json.dumps(before))
        recovered = SubmitRate(self.root, Profile())
        self.assertGreater(recovered.resume_at, 0)
        recovered.settled()
        history = json.loads(limiter.path.read_text())
        self.assertEqual(history[:3], [recovered.host, recovered.clock_origin, recovered.period])
        self.assertEqual(len(history[3]), 1)
        self.assertIs(history[4], False)

    def test_invalid_submission_history_does_not_reset_the_call_allowance(self):
        path = self.root / "submit-rate.json"
        for text in (
            '["host",NaN,60,[],false]',
            '["host",0,Infinity,[],false]',
            '["host",0,0,[],false]',
            '["host",0,60,[2,1],false]',
            '["host",0,60,[true],false]',
            '["host",false,60,[],false]',
            '["host",0,60,[],1]',
            "[]",
            "null",
            "{",
        ):
            with self.subTest(text=text):
                path.write_text(text)
                with self.assertRaises(DagError):
                    SubmitRate(self.root, Profile())

    def test_existing_retry_records_keep_the_attempt_and_allow_zero_resource_values(self):
        job = read(sample(self.root)).jobs[0]
        path = self.root / "attempts.json"
        path.write_text(json.dumps({job.work.key: [job.work.fingerprint, 2, 0, 0]}))
        attempts = Attempts(path)
        self.assertEqual(attempts.resume(job, 1024, 60), (2, 0, 0))
        attempts.set(job, 3, 2048, 120.0)
        self.assertEqual(json.loads(path.read_text())[job.work.key], [job.work.fingerprint, 3, 2048, 120.0])
        self.assertEqual(Attempts(path).resume(job, 1024, 60), (3, 2048, 120.0))
        attempts.done(job)
        self.assertEqual(Attempts(path).resume(job, 1024, 60), (1, 1024, 60))

    def test_corrupt_or_unreadable_retry_history_cannot_restart_the_attempt_count(self):
        path = self.root / "attempts.json"
        for text in (
            "",
            "null",
            "[]",
            '{"a":["fingerprint",0,1,1]}',
            '{"a":["fingerprint",true,1,1]}',
            '{"a":["fingerprint",2,-1,1]}',
            '{"a":["fingerprint",2,1,NaN]}',
            '{"a":["fingerprint",2,1]}',
        ):
            with self.subTest(text=text):
                path.write_text(text)
                with self.assertRaisesRegex(DagError, "cannot verify Slurm retry history"):
                    Attempts(path)
        with patch.object(Path, "read_bytes", side_effect=PermissionError("not readable")):
            with self.assertRaises(DagError):
                Attempts(path)


if __name__ == "__main__":
    unittest.main()
