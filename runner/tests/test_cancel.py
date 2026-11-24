import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from spit_bash.slurm import commands

# A scancel that cancels the IDs it is given but exits 1 when any is not a job, as Slurm's does, and that
# refuses 30_2 for a reason of its own.
SCANCEL = """#!/bin/sh
status=0
for id in "$@"; do
  case "$id" in
    9_*) echo "scancel: error: Kill job error on job id $id: Invalid job id specified" >&2; status=1 ;;
    30_2) echo "scancel: error: Access/permission denied" >&2; status=1 ;;
    *) echo "$id" >> "$CANCELLED" ;;
  esac
done
exit $status
"""


class CancelReportTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        folder = Path(directory.name)
        program = folder / "scancel"
        program.write_text(SCANCEL)
        program.chmod(program.stat().st_mode | stat.S_IXUSR)
        self.cancelled = folder / "cancelled"
        patch = mock.patch.dict(os.environ, {"PATH": f"{folder}:{os.environ['PATH']}", "CANCELLED": str(self.cancelled)})
        patch.start()
        self.addCleanup(patch.stop)

    def done(self) -> list[str]:
        return self.cancelled.read_text().split() if self.cancelled.exists() else []

    def test_one_ended_task_does_not_hide_the_others(self):
        failed = commands.cancel_report(["30_0", "9_1", "30_1"])
        self.assertEqual(failed, {})
        # The retry asks again for the tasks the first call already cancelled, which does no harm.
        self.assertEqual(sorted(set(self.done())), ["30_0", "30_1"])

    def test_a_refused_task_is_reported_with_scancels_message(self):
        failed = commands.cancel_report(["30_0", "30_2", "9_1"])
        self.assertEqual(failed, {"30_2": "Access/permission denied"})

    def test_cancel_lists_the_failed_tasks(self):
        self.assertEqual(commands.cancel(["30_2", "30_0"]), ["30_2"])

    def test_a_scancel_that_does_not_answer_is_not_tried_task_by_task(self):
        with mock.patch.object(commands.subprocess, "run", side_effect=commands.subprocess.TimeoutExpired("scancel", 10)) as run:
            failed = commands.cancel_report(["1_0", "1_1", "1_2"])
        self.assertEqual(failed, dict.fromkeys(["1_0", "1_1", "1_2"], commands.UNANSWERED))
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
