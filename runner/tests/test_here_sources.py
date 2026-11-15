"""A `here` source is read from the pipeline's folder, as an input, and no job writes there."""

import shutil
import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.plan import Status, plan
from support import UPPER, artifact, document, job_entry, read, run, state_at


class HereSourceTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.folder = Path(tempfile.mkdtemp())
        for folder in (self.root, self.folder):
            self.addCleanup(shutil.rmtree, folder, True)
        (self.folder / "scripts").mkdir()
        (self.folder / "scripts" / "upper.py").write_text(UPPER)
        (self.root / "source.txt").write_text("a")

    def data(self, output="upper.txt"):
        entry = job_entry(1, ["source.txt"], output, "")
        entry["inputs"]["script"] = [artifact("scripts/upper.py", "pipeline_folder")]
        entry["commands"][0]["command"] = [["python3"], [{"path": "scripts/upper.py"}],
                                           [{"path": "source.txt"}], [{"path": output}]]
        data = document(self.root, [entry], ["source.txt"])
        data["external_inputs"].append(artifact("scripts/upper.py", "pipeline_folder"))
        data["pipeline_folder"] = str(self.folder)
        return data

    def test_a_job_runs_its_here_source_and_reruns_when_it_changes(self):
        data = self.data()
        run(read(data), state_at(self.root))
        self.assertEqual((self.root / "upper.txt").read_text(), "A")
        self.assertEqual([d.status for d in plan(read(data), state_at(self.root))], [Status.SKIP])
        (self.folder / "scripts" / "upper.py").write_text(UPPER + "\n")
        self.assertEqual([d.status for d in plan(read(data), state_at(self.root))], [Status.RUN])

    def test_a_dag_whose_job_writes_under_the_pipeline_folder_is_refused(self):
        data = self.data()
        data["jobs"][0]["outputs"]["output"]["under"] = "pipeline_folder"
        with self.assertRaisesRegex(DagError, "under the pipeline's folder, which jobs only read"):
            read(data)

    def test_a_here_source_needs_the_pipeline_folder(self):
        data = self.data()
        data["pipeline_folder"] = str(self.folder / "gone")
        with self.assertRaisesRegex(DagError, "from the pipeline's folder, which is not a directory"):
            read(data)

    def test_a_link_out_of_the_pipeline_folder_is_refused(self):
        outside = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, outside, True)
        (outside / "upper.py").write_text(UPPER)
        (self.folder / "scripts" / "upper.py").unlink()
        (self.folder / "scripts" / "upper.py").symlink_to(outside / "upper.py")
        with self.assertRaisesRegex(DagError, "outside the pipeline root"):
            read(self.data())


if __name__ == "__main__":
    unittest.main()
