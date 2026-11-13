import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from support import artifact, document, job_entry, UPPER, state_at
from test_cli import call


class CleanTests(unittest.TestCase):
    """`clean` lists, and with --yes deletes, what a folder of outputs holds that the DAG does not explain."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        (self.root / "source.txt").write_text("hello")
        self.data = document(self.root, [
            job_entry(1, ["source.txt"], "out/upper/sub=01.txt", UPPER, dependents=[2]),
            job_entry(2, ["out/upper/sub=01.txt"], "out/final/sub=01.txt", UPPER, depends_on=[1]),
        ], ["source.txt"])
        for job in self.data["jobs"]:
            job["outputs"]["output"]["entities"] = {"sub": "01"}
        self.dag = self.root / "jobs.spitdag"
        self.write()
        code, _, err = call("run", str(self.dag))
        self.assertEqual(code, 0, err)
        self.out = self.root / "out"

    def write(self):
        self.dag.write_text(json.dumps(self.data))

    def test_a_folder_of_explained_outputs_has_no_strays(self):
        code, out, err = call("clean", str(self.dag), str(self.out))
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "everything under out is explained by the DAG\n")

    def test_it_lists_strays_grouped_by_entities_the_dag_lacks_and_deletes_nothing(self):
        (self.out / "upper" / "map_real.txt").write_text("1234")
        (self.out / "upper" / "sub=03.txt").write_text("12")
        (self.out / "old" / "deep").mkdir(parents=True)
        (self.out / "old" / "deep" / "a.txt").write_text("123")
        code, out, err = call("clean", str(self.dag), str(self.out))
        self.assertEqual(code, 0, err)
        self.assertEqual(out.splitlines(), [
            "3 paths (9 bytes) under out that the DAG does not explain:",
            "  out/old/ (3 bytes)",
            "  out/upper/map_real.txt (4 bytes)",
            "  sub=03, which no job of the DAG reads or makes:",
            "    out/upper/sub=03.txt (2 bytes)",
            "run again with --yes to delete them",
        ])
        self.assertTrue((self.out / "old" / "deep" / "a.txt").exists())

    def test_yes_deletes_them_and_forgets_the_records_of_outputs_it_deleted(self):
        self.data["jobs"] = self.data["jobs"][:1]
        self.data["jobs"][0]["dependents"] = []
        self.write()
        (self.out / "stray.txt").write_text("x")
        code, out, err = call("clean", str(self.dag), str(self.out), "--yes")
        self.assertEqual(code, 0, err)
        self.assertEqual(out.splitlines()[0], "deleting 2 paths (6 bytes) under out that the DAG does not explain:")
        self.assertEqual(sorted(path.name for path in self.out.iterdir()), ["upper"])
        self.assertTrue((self.out / "upper" / "sub=01.txt").exists())
        self.assertTrue((self.root / ".spit-bash" / "state.jsonl").exists())
        state = state_at(self.root)
        self.assertEqual(len(state.jobs), 1)
        state.close()

    def test_the_runner_folder_and_edited_copies_are_explained(self):
        (self.out / "upper" / "sub=01.txt.edited-20260101T000000").write_text("mine")
        code, out, err = call("clean", str(self.dag), str(self.root / "out"), "--dir", str(self.out / ".runner"))
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "everything under out is explained by the DAG\n")

    def test_a_folder_holding_a_source_is_refused(self):
        code, _, err = call("clean", str(self.dag), str(self.root))
        self.assertEqual(code, 2)
        self.assertIn("holds the source source.txt, so it is not a folder of outputs only", err)

    def test_a_folder_no_job_writes_to_is_refused(self):
        (self.root / "notes").mkdir()
        code, _, err = call("clean", str(self.dag), str(self.root / "notes"))
        self.assertEqual(code, 2)
        self.assertIn("no job writes under", err)

    def test_a_folder_outside_the_roots_is_refused(self):
        with tempfile.TemporaryDirectory() as elsewhere:
            code, _, err = call("clean", str(self.dag), elsewhere)
        self.assertEqual(code, 2)
        self.assertIn("is under neither the dataset root", err)

    def test_a_partial_dag_or_a_selection_is_refused(self):
        code, _, err = call("clean", str(self.dag), str(self.out), "--product", "out/upper/sub=01.txt")
        self.assertEqual(code, 2)
        self.assertIn("clean takes the whole DAG", err)
        self.data["left_out"] = [{"identity": "final[]", "reasons": ["missing input"]}]
        self.write()
        code, _, err = call("clean", str(self.dag), str(self.out))
        self.assertEqual(code, 2)
        self.assertIn("cannot clean with a partial DAG", err)

    def test_a_stray_it_cannot_delete_is_reported_and_the_rest_deleted(self):
        (self.out / "locked").mkdir()
        (self.out / "gone.txt").write_text("x")
        refused = PermissionError(13, "Permission denied")
        with mock.patch("spit_bash.clean.shutil.rmtree", side_effect=refused):
            code, _, err = call("clean", str(self.dag), str(self.out), "--yes")
        self.assertEqual(code, 1)
        self.assertEqual(err, f"could not delete {self.out / 'locked'}: Permission denied\n")
        self.assertFalse((self.out / "gone.txt").exists())

    def test_the_listing_says_which_strays_cannot_be_deleted(self):
        (self.out / "gone.txt").write_text("x")
        with mock.patch("spit_bash.clean.os.access", return_value=False):
            _, out, _ = call("clean", str(self.dag), str(self.out))
        self.assertIn("  out/gone.txt (1 bytes, cannot be deleted: its folder is not writable)", out)

    def test_strays_under_the_output_root_are_shown_from_it(self):
        results = self.root / "results"
        self.data["output_root"] = str(results)
        for job in self.data["jobs"]:
            job["outputs"]["output"] = dict(job["outputs"]["output"], under="output_root")
        self.data["jobs"][1]["inputs"]["input"][0]["under"] = "output_root"
        self.write()
        (results / "out").mkdir(parents=True)
        (results / "out" / "x.txt").write_text("x")
        code, out, err = call("clean", str(self.dag), str(results))
        self.assertEqual(code, 0, err)
        self.assertIn("  out/x.txt (1 bytes)", out)


if __name__ == "__main__":
    unittest.main()
