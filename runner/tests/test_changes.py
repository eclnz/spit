import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from support import UPPER, document, job_entry

SOURCE = Path(__file__).resolve().parents[1] / "src"
FAILS = "raise SystemExit(1)"


class ChangeSummaryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.env = dict(os.environ, PYTHONPATH=str(SOURCE))
        for name in ("reference.txt", "sample.txt"):
            (self.root / name).write_text("unchanged input\n")

    def write(self, names, scripts):
        jobs = [job_entry(number, [name], f"{name[:-4]}-output.txt", script)
                for number, (name, script) in enumerate(zip(names, scripts), 1)]
        data = document(self.root, jobs, names)
        artifacts = list(data["external_inputs"])
        artifacts += [a for job in data["jobs"] for port in job["inputs"].values() for a in port]
        for artifact in artifacts:
            artifact["product"] = "reference" if artifact["path"] == "reference.txt" else "measurement"
        self.dag = self.root / "example.spitdag"
        self.dag.write_text(json.dumps(data))

    def spit_bash(self, action, *args):
        result = subprocess.run([sys.executable, "-m", "spit_bash", action, str(self.dag), *args],
                                env=self.env, capture_output=True, text=True)
        return result.stdout + result.stderr

    def summary(self, *args):
        lines = [line for line in self.spit_bash("plan", *args).splitlines() if line.startswith("since the last run")]
        return lines[0] if lines else None

    def test_a_failed_job_does_not_make_its_unchanged_input_look_added(self):
        self.write(["reference.txt", "sample.txt"], [UPPER, FAILS])
        self.assertIn("1 failed", self.spit_bash("run"))
        for _ in range(2):
            self.assertIsNone(self.summary())

    def test_a_job_that_never_ran_does_not_make_its_input_look_added(self):
        self.write(["reference.txt", "sample.txt"], [UPPER, UPPER])
        self.spit_bash("run", "--product", "reference")
        self.assertIsNone(self.summary())

    def test_an_added_input_is_reported_until_a_run_has_seen_it(self):
        self.write(["reference.txt"], [UPPER])
        self.assertIn("1 done", self.spit_bash("run"))
        (self.root / "sample.txt").write_text("new\n")
        self.write(["reference.txt", "sample.txt"], [UPPER, FAILS])
        self.assertEqual(self.summary(), "since the last run: +1 measurement")
        self.spit_bash("run")
        self.assertIsNone(self.summary())

    def test_an_edited_input_is_reported_after_a_failed_run(self):
        self.write(["reference.txt", "sample.txt"], [UPPER, FAILS])
        self.spit_bash("run")
        (self.root / "reference.txt").write_text("edited\n")
        self.assertEqual(self.summary(), "since the last run: 1 reference changed")


if __name__ == "__main__":
    unittest.main()
