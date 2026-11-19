"""The call log counts and tags Slurm commands without changing what a run does."""

import json
import tempfile
import unittest
from pathlib import Path

from spit_bash.slurm import calllog


class CallLogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.addCleanup(setattr, calllog, "active", None)
        self.path = Path(tempfile.mkdtemp()) / "slurm" / "call_logs.json"

    def runs(self) -> list[dict]:
        return json.loads(self.path.read_text())["runs"]

    def test_counts_each_tool_and_reason(self) -> None:
        calllog.start(self.path)
        calllog.record("squeue", "routine")
        calllog.record("squeue", "start")
        calllog.record("squeue", "start")
        calllog.record("sbatch", "submit")
        totals = self.runs()[-1]["totals"]
        self.assertEqual(totals["squeue"], 3)
        self.assertEqual(totals["squeue:start"], 2)
        self.assertEqual(totals["squeue:routine"], 1)
        self.assertEqual(totals["sbatch"], 1)
        self.assertEqual([call[1:] for call in self.runs()[-1]["calls"]][:2],
                         [["squeue", "routine"], ["squeue", "start"]])

    def test_without_a_run_nothing_is_written(self) -> None:
        calllog.record("squeue", "routine")
        self.assertFalse(self.path.exists())

    def test_each_run_adds_a_record_and_old_ones_are_kept_to_a_limit(self) -> None:
        for _ in range(calllog.MAX_RUNS + 3):
            calllog.start(self.path).record("sacct", "classify")
        self.assertEqual(len(self.runs()), calllog.MAX_RUNS)

    def test_timeline_is_capped_but_totals_stay_exact(self) -> None:
        log = calllog.start(self.path)
        log.calls = [[0.0, "squeue", "routine"]] * calllog.MAX_CALLS
        log.record("squeue", "routine")
        run = self.runs()[-1]
        self.assertEqual(len(run["calls"]), calllog.MAX_CALLS)
        self.assertEqual((run["dropped"], run["totals"]["squeue"]), (1, 1))

    def test_a_damaged_file_does_not_stop_a_run(self) -> None:
        self.path.parent.mkdir(parents=True)
        self.path.write_text("not json")
        calllog.start(self.path).record("squeue", "routine")
        self.assertEqual(len(self.runs()), 1)


if __name__ == "__main__":
    unittest.main()
