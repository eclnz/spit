import os
import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.plan import Status, plan
from support import artifact, command, document, error, read, run, state_at

APPEND = "from pathlib import Path; import sys\np = Path(sys.argv[1]) / 'a.txt'; p.write_text(p.read_text() + 'more')\n"
COPY = "from pathlib import Path; import sys; Path(sys.argv[2]).write_text(Path(sys.argv[1]).read_text())"
LIST = (
    "from pathlib import Path; import sys\n"
    "Path(sys.argv[2]).write_text(' '.join(sorted(p.name for p in Path(sys.argv[1]).iterdir())))\n"
)


def folder(path, under=None):
    return dict(artifact(path, under), kind="folder")


def seeded_dag(root):
    data = document(
        root,
        [
            {
                "id": 1,
                "operation": "edit",
                "stage": [],
                "dependents": [],
                "inputs": {"input": [folder("pages")]},
                "outputs": {"output": folder("work")},
                "seed_folders": {"output": "input"},
                "depends_on": [],
                "verify": [],
                "checks": [],
                "origin": None,
                "cores": 1,
                "memory": None,
                "commands": [{"command": command(APPEND, "work"), "stdin": None, "stdout": None}],
            }
        ],
        [],
    )
    data["external_inputs"] = [folder("pages")]
    return data


def with_ids(item, id):
    return dict(item, id=id)


def view_dag(root):
    a, b = with_ids(artifact("a.txt"), 0), with_ids(artifact("b.txt"), 1)
    made_a = with_ids(artifact("out.anat/a.txt", "output_root"), 2)
    made_b = with_ids(artifact("out.anat/b.txt", "output_root"), 3)
    view = {
        "product": "anat",
        "entities": {},
        "type": None,
        "path": "out.anat",
        "under": "output_root",
        "id": 4,
        "kind": "directory_view",
        "members": [{"id": 2, "path": "a.txt"}, {"id": 3, "path": "b.txt"}],
    }
    listed = with_ids(artifact("list.txt", "output_root"), 5)

    def copy(id, source, made):
        return {
            "id": id,
            "operation": "copy",
            "stage": [],
            "dependents": [3],
            "inputs": {"input": [source]},
            "outputs": {"output": made},
            "depends_on": [],
            "verify": [],
            "checks": [],
            "origin": None,
            "cores": 1,
            "memory": None,
            "commands": [{"command": command(COPY, source["path"], made["path"]), "stdin": None, "stdout": None}],
        }

    data = document(
        root,
        [
            copy(1, a, made_a),
            copy(2, b, made_b),
            {
                "id": 3,
                "operation": "list",
                "stage": [],
                "dependents": [],
                "inputs": {"anat": [view]},
                "outputs": {"output": listed},
                "depends_on": [1, 2],
                "verify": [],
                "checks": [],
                "origin": None,
                "cores": 1,
                "memory": None,
                "commands": [{"command": command(LIST, "out.anat", "list.txt"), "stdin": None, "stdout": None}],
            },
        ],
        [],
    )
    data["external_inputs"] = [a, b]
    data["targets"] = [listed]
    return data


class SeedTests(unittest.TestCase):
    def test_an_output_folder_starts_as_a_copy_its_command_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "pages/deep").mkdir(parents=True)
            (root / "pages/a.txt").write_text("one ")
            (root / "pages/deep/b.txt").write_text("two")
            os.symlink("deep/b.txt", root / "pages/link")
            dag = read(seeded_dag(root))
            _, results = run(dag, state_at(root))
            self.assertFalse(any(error(r) for r in results), [error(r) for r in results])
            self.assertEqual((root / "work/a.txt").read_text(), "one more")
            self.assertEqual((root / "work/deep/b.txt").read_text(), "two")
            self.assertEqual(os.readlink(root / "work/link"), "deep/b.txt")
            self.assertEqual((root / "pages/a.txt").read_text(), "one ")
            run(dag, state_at(root), force=True)
            self.assertEqual((root / "work/a.txt").read_text(), "one more")

    def test_a_link_out_of_the_input_folder_fails_the_job(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "pages").mkdir()
            (root / "pages/a.txt").write_text("one ")
            os.symlink("../outside.txt", root / "pages/escape")
            _, results = run(read(seeded_dag(root)), state_at(root))
            self.assertIn("escape", error(results[0]))
            self.assertIn("outside the folder", error(results[0]))
            self.assertFalse((root / "work").exists())

    def test_a_seed_names_an_output_folder_and_an_input_of_one_folder(self):
        data = seeded_dag(None)
        data["jobs"][0]["seed_folders"] = {"output": "missing"}
        with self.assertRaisesRegex(DagError, "not an input of one folder"):
            read(data, "/")
        data["jobs"][0]["seed_folders"] = {"other": "input"}
        with self.assertRaisesRegex(DagError, "not one of its output folders"):
            read(data, "/")

    def test_the_seed_is_part_of_the_work(self):
        plain = seeded_dag(None)
        del plain["jobs"][0]["seed_folders"]
        self.assertNotEqual(
            read(plain, "/").jobs[0].work.fingerprint, read(seeded_dag(None), "/").jobs[0].work.fingerprint
        )


class ViewTests(unittest.TestCase):
    def test_a_view_is_read_through_its_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.txt").write_text("a")
            (root / "b.txt").write_text("b")
            dag = read(view_dag(root))
            reader = dag.jobs[2]
            self.assertEqual(reader.input_paths, ("out.anat/a.txt", "out.anat/b.txt"))
            _, results = run(dag, state_at(root))
            self.assertFalse(any(error(r) for r in results), [error(r) for r in results])
            self.assertEqual((root / "list.txt").read_text(), "a.txt b.txt")
            self.assertEqual([d.status for d in plan(dag, state_at(root))], [Status.SKIP] * 3)
            (root / "b.txt").write_text("changed")
            statuses = [d.status for d in plan(dag, state_at(root))]
            self.assertEqual(statuses[:2], [Status.SKIP, Status.RUN])
            self.assertNotEqual(statuses[2], Status.SKIP)
            decisions, results = run(dag, state_at(root))
            self.assertEqual([r.job.id for r in results if r.failure is None], [2, 3])
            self.assertEqual([d.status for d in plan(dag, state_at(root))], [Status.SKIP] * 3)

    def test_a_member_must_be_a_file_a_job_writes_inside_the_view(self):
        data = view_dag(None)
        data["jobs"][2]["inputs"]["anat"][0]["members"][1]["path"] = "c.txt"
        with self.assertRaisesRegex(DagError, "names c.txt, but artifact 3 is at out.anat/b.txt"):
            read(data, "/")
        data = view_dag(None)
        data["jobs"][2]["inputs"]["anat"][0]["members"][1]["id"] = 0
        with self.assertRaisesRegex(DagError, "which is no file a job writes"):
            read(data, "/")

    def test_the_names_of_a_views_files_are_part_of_the_work(self):
        renamed = view_dag(None)
        renamed["jobs"][1]["outputs"]["output"]["path"] = "out.anat/c.txt"
        renamed["jobs"][2]["inputs"]["anat"][0]["members"][1]["path"] = "c.txt"
        self.assertNotEqual(
            read(renamed, "/").jobs[2].work.fingerprint, read(view_dag(None), "/").jobs[2].work.fingerprint
        )


if __name__ == "__main__":
    unittest.main()
