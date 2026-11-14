"""Tools change work, while explicit acceptance retains file evidence."""

from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from spit_bash import tools
from spit_bash.model import DagError
from spit_bash.plan import plan, Status
from support import read, run, sample, state_at


class ToolTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        (self.root / "source.txt").write_text("hello")

    def data(self, identity="one", probe=None):
        data = sample(self.root)
        data["tools"] = [{"name": "compiler", "identity": None if probe else identity, "probe": probe}]
        for job in data["jobs"]:
            job["tools"] = ["compiler"]
        return data

    def test_a_probe_runs_once_and_its_stdout_is_byte_exact(self):
        command = [sys.executable, "-c", "import sys;sys.stdout.buffer.write(b'v1\\x00\\n')"]
        with patch.object(tools.subprocess, "run", wraps=tools.subprocess.run) as execute:
            dag = read(self.data(probe=command))
        self.assertEqual(execute.call_count, 1)
        self.assertEqual(dag.jobs[0].work.tools, dag.jobs[1].work.tools)
        self.assertEqual(tools.shown(dag.jobs[0].work.tools["compiler"]), "v1\x00\n")

    def test_a_failed_probe_starts_no_job(self):
        with self.assertRaisesRegex(DagError, "tool probe compiler exited 7"):
            read(self.data(probe=[sys.executable, "-c", "raise SystemExit(7)"]))
        self.assertFalse((self.root / "work").exists())

    def test_a_tool_change_reruns_work_and_acceptance_does_not_adopt_an_edited_input(self):
        first = read(self.data())
        run(first, state_at(self.root))
        second = read(self.data("two"))
        state = state_at(self.root)
        self.addCleanup(state.close)
        self.assertIsNot(plan(second, state)[0].status, Status.SKIP)
        self.assertTrue(any("compiler identity changed" in line for line in tools.changed_notes(second, state)))
        tools.accept(second, state)
        self.assertTrue(all(d.status is Status.SKIP for d in plan(second, state)))
        (self.root / "source.txt").write_text("edited")
        third = read(self.data("three"))
        tools.accept(third, state)
        self.assertIsNot(plan(third, state)[0].status, Status.SKIP)

    def test_acceptance_refuses_other_changed_work_before_writing(self):
        run(read(self.data()), state_at(self.root))
        data = self.data("two")
        data["jobs"][1]["commands"][0]["command"].append(["new-argument"])
        state = state_at(self.root)
        self.addCleanup(state.close)
        before = dict(state.jobs)
        with self.assertRaisesRegex(DagError, "other than tools changed"):
            tools.accept(read(data), state)
        self.assertEqual(state.jobs, before)

    def test_a_local_input_edit_is_seen_independently_of_the_dataset_root(self):
        pipeline = self.root / "pipeline"
        pipeline.mkdir()
        script = pipeline / "source.txt"
        script.write_text("one")
        data = sample(self.root)
        data["pipeline_folder"] = str(pipeline)
        for artifact in [*data["external_inputs"], *data["jobs"][0]["inputs"]["input"]]:
            artifact["under"] = "pipeline_folder"
        dag = read(data)
        run(dag, state_at(self.root))
        self.assertTrue(all(d.status is Status.SKIP for d in plan(dag, state_at(self.root))))
        script.write_text("two")
        self.assertIsNot(plan(dag, state_at(self.root))[0].status, Status.SKIP)


if __name__ == "__main__":
    unittest.main()
