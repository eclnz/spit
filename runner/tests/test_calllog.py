import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from spit_bash.slurm import calllog


class CallLogTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / "slurm" / "call_logs.jsonl"

    def entries(self) -> list[dict]:
        return [json.loads(line) for line in self.path.read_text().splitlines()]

    def test_counts_are_visible_during_the_run_and_final_on_exit(self) -> None:
        with calllog.capture(self.path), patch.object(calllog.time, "monotonic", side_effect=[1, 2, 61, 62]):
            calllog.record(calllog.Command.QUEUE, "routine")
            first = self.path.read_bytes()
            self.assertFalse(self.entries()[0]["final"])
            calllog.record(calllog.Command.QUEUE, "start")
            self.assertEqual(self.path.read_bytes(), first)
            calllog.record(calllog.Command.QUEUE, "start")
            self.assertEqual(self.entries()[-1]["totals"]["squeue:start"], 2)
            calllog.record(calllog.Command.SUBMIT, "submit")
        self.assertTrue(self.path.read_bytes().startswith(first))
        entries = self.entries()
        self.assertEqual([entry["final"] for entry in entries], [False, False, True])
        self.assertEqual(
            entries[-1]["totals"],
            {"squeue": 3, "squeue:routine": 1, "squeue:start": 2, "sbatch": 1, "sbatch:submit": 1},
        )
        self.assertEqual(sum(len(entry["calls"]) for entry in entries), 4)
        calllog.record(calllog.Command.QUEUE, "routine")
        self.assertEqual(self.entries(), entries)

    def test_without_a_run_nothing_is_written(self) -> None:
        calllog.record(calllog.Command.QUEUE, "routine")
        self.assertFalse(self.path.exists())

    def test_each_run_appends_and_preserves_earlier_records(self) -> None:
        for _ in range(2):
            with calllog.capture(self.path):
                calllog.record(calllog.Command.ACCOUNTING, "classify")
        self.assertEqual([entry["final"] for entry in self.entries()], [False, True, False, True])

    def test_timeline_is_capped_but_totals_stay_exact(self) -> None:
        with calllog.capture(self.path):
            for _ in range(calllog.MAX_CALLS + 1):
                calllog.record(calllog.Command.QUEUE, "routine")
        entries = self.entries()
        self.assertEqual(sum(len(entry["calls"]) for entry in entries), calllog.MAX_CALLS)
        self.assertEqual(entries[-1]["dropped"], 1)
        self.assertEqual(entries[-1]["totals"]["squeue"], calllog.MAX_CALLS + 1)

    def test_a_write_failure_does_not_stop_a_run(self) -> None:
        self.path.mkdir(parents=True)
        with calllog.capture(self.path):
            calllog.record(calllog.Command.QUEUE, "routine")
