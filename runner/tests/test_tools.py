"""A tool's identity is part of the work of each job that depends on it: a changed tool reruns those jobs alone."""

import contextlib
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

from spit_bash import cli
from spit_bash.model import DagError
from spit_bash.plan import Status, plan
from support import read, run, sample, state_at

# A probe that prints the version file under the dataset root, which it runs from.
PROBE = [sys.executable, "-c", "print(open('version.txt').read(), end='')"]


class ToolTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        (self.root / "source.txt").write_text("hello\n")
        (self.root / "version.txt").write_text("tool 1.0\nbuild a\n")
        self.data = sample(self.root)
        self.data["tools"] = [{"name": "upper", "identity": None, "probe": PROBE},
                              {"name": "image", "identity": "sha256:1", "probe": None}]
        self.data["jobs"][0]["tools"] = ["upper"]
        self.data["jobs"][1]["tools"] = ["image"]
        self.path = self.root / "plan.spitdag"
        self.write()

    def write(self):
        self.path.write_text(json.dumps(self.data))

    def cli(self, *args):
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(err):
            status = cli.main([*args, str(self.path)])
        return status, out.getvalue(), err.getvalue()

    def decisions(self):
        state = state_at(self.root)
        try:
            return {d.job.id: d for d in plan(read(self.data), state)}
        finally:
            state.close()

    def test_a_changed_probe_reruns_only_the_jobs_that_use_its_tool(self):
        status, _, _ = self.cli("run")
        self.assertEqual(status, 0)
        (self.root / "version.txt").write_text("tool 1.0\nbuild b\n")
        decisions = self.decisions()
        self.assertEqual(decisions[1].status, Status.RUN)
        self.assertEqual(decisions[1].cause.parts, ("tool:upper",))
        # Job 2 reads job 1's output, so it may run, but its own work is the same.
        self.assertEqual(decisions[2].status, Status.MAYBE)
        status, out, err = self.cli("plan")
        self.assertIn("tool upper changed since it was last recorded:\n  -build a\n  +build b", err)
        self.assertIn("since the last run: op1: tool upper changed (1 job)", out)

    def test_a_declared_identity_is_compared_as_written(self):
        self.cli("run")
        self.data["tools"][1]["identity"] = "sha256:2"
        self.write()
        decisions = self.decisions()
        self.assertEqual(decisions[1].status, Status.SKIP)
        self.assertEqual(decisions[2].status, Status.RUN)
        self.assertEqual(decisions[2].cause.parts, ("tool:image",))

    def test_accept_tools_records_the_new_identities_without_rerunning(self):
        self.cli("run")
        (self.root / "version.txt").write_text("tool 2.0\n")
        status, _, err = self.cli("accept-tools")
        self.assertEqual(status, 0)
        self.assertIn("recorded the tools as they are now for 1 job", err)
        self.assertTrue(all(d.status is Status.SKIP for d in self.decisions().values()))
        _, _, err = self.cli("plan")
        self.assertNotIn("changed since it was last recorded", err)

    def test_a_job_whose_work_changed_otherwise_is_not_accepted(self):
        self.cli("run")
        (self.root / "version.txt").write_text("tool 2.0\n")
        self.data["jobs"][0]["commands"][0]["command"][2] = ["print('other')"]
        self.write()
        _, _, err = self.cli("accept-tools")
        self.assertIn("for 0 jobs", err)

    def test_a_failed_probe_stops_before_any_job(self):
        self.data["tools"][0]["probe"] = [sys.executable, "-c", "import sys; sys.exit('no licence')"]
        with self.assertRaisesRegex(DagError, "the probe of tool upper, .* exited with status 1: no licence"):
            read(self.data)
        self.data["tools"][0]["probe"] = ["no-such-program-spit"]
        with self.assertRaisesRegex(DagError, "cannot run the probe of tool upper"):
            read(self.data)

    def test_a_job_names_only_listed_tools(self):
        self.data["jobs"][0]["tools"] = ["missing"]
        with self.assertRaisesRegex(DagError, "job 1 names tool missing"):
            read(self.data)

    def test_a_dag_without_tools_identifies_work_as_before(self):
        plain = sample(self.root)
        self.assertNotIn("tool:upper", read(plain).jobs[0].work.parts)
        self.assertIn("tool:upper", read(self.data).jobs[0].work.parts)

    def test_a_job_run_recorded_with_the_tools_is_current(self):
        state = state_at(self.root)
        run(read(self.data), state)
        self.assertTrue(all(d.status is Status.SKIP for d in self.decisions().values()))


if __name__ == "__main__":
    unittest.main()
