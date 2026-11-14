"""Collection arguments preserve paths, roots and temporary-file ownership."""

import json
from pathlib import Path
import shutil
import tempfile
import unittest

from spit_bash.model import DagError
from spit_bash.plan import plan, Status
from support import command, document, error, job_entry, read, run, state_at


class CollectionTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name in ["two words.txt", "10.txt"]:
            (self.root / name).write_text(name)

    def data(self, part, script=None):
        script = script or "import json,sys; from pathlib import Path; Path(sys.argv[-1]).write_text(json.dumps(sys.argv[1:-1]))"
        item = job_entry(1, ["two words.txt", "10.txt"], "out.txt", script)
        item["commands"][0]["command"] = command(script) + [["--in=", part], [{"path": "out.txt"}]]
        return document(self.root, [item], ["two words.txt", "10.txt"])

    def test_empty_and_single_member_collections_preserve_one_argument(self):
        for members in [[], ["two words.txt"]]:
            for separator in ["", ",", "\" "]:
                dag = read(self.data({"join": members, "separator": separator}))
                _, results = run(dag, state_at(self.root), force=True)
                self.assertEqual([error(r) for r in results], [None])
                self.assertEqual(json.loads((self.root / "out.txt").read_text()), ["--in=" + separator.join(members)])

    def test_list_files_are_removed_after_a_command_fails(self):
        script = "import sys;from pathlib import Path;p=sys.argv[1].removeprefix('--in=');Path('seen.txt').write_text(p);raise SystemExit(3)"
        dag = read(self.data({"list": ["two words.txt"]}, script))
        _, results = run(dag, state_at(self.root))
        self.assertIsNotNone(error(results[0]))
        self.assertFalse(Path((self.root / "seen.txt").read_text()).exists())

    def test_a_join_is_one_argument_with_literal_spaces(self):
        dag = read(self.data({"join": ["two words.txt", "10.txt"], "separator": ","}))
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(r) for r in results], [None])
        self.assertEqual(json.loads((self.root / "out.txt").read_text()), ["--in=two words.txt,10.txt"])

    def test_a_list_is_shared_with_verify_and_removed_after_the_job(self):
        script = "import sys,json; from pathlib import Path; p=Path(sys.argv[1].removeprefix('--in=')); Path(sys.argv[-1]).write_text(json.dumps([str(p),p.read_text()]))"
        part = {"list": ["two words.txt", "10.txt"]}
        data = self.data(part, script)
        data["jobs"][0]["verify"] = [[["test"], ["-f"], [part]]]
        dag = read(data)
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(r) for r in results], [None])
        temporary, contents = json.loads((self.root / "out.txt").read_text())
        self.assertEqual(contents, "".join(str(self.root / p) + "\n" for p in part["list"]))
        self.assertFalse(Path(temporary).exists())
        # A different temporary filename on a later run cannot change the key.
        self.assertEqual(dag.jobs[0].work, read(data).jobs[0].work)
        self.assertTrue(all(p.status is Status.SKIP for p in plan(dag, state_at(self.root))))

    def test_a_join_checks_the_effective_root_and_its_override(self):
        other = self.root / "data,with-comma"
        other.mkdir()
        for name in ["two words.txt", "10.txt"]:
            (other / name).write_text("x")
        data = self.data({"join": ["two words.txt", "10.txt"], "separator": ","})
        data["output_root"] = str(self.root / "results")
        with self.assertRaisesRegex(DagError, "join separator"):
            read(data, str(other))

    def test_collection_paths_must_be_inputs(self):
        for part in [{"list": ["out.txt"]}, {"join": ["missing"], "separator": ","}]:
            with self.assertRaisesRegex(DagError, "outside its inputs"):
                read(self.data(part))

    def test_an_anchored_job_reruns_after_a_whole_dataset_move(self):
        data = self.data({"join": ["two words.txt", "10.txt"], "separator": ","})
        data["jobs"][0]["anchored"] = True
        dag = read(data)
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(r) for r in results], [None])
        moved = self.root / "moved"
        shutil.copytree(self.root, moved, ignore=shutil.ignore_patterns("moved"))
        data["root"] = data["output_root"] = str(moved)
        new = read(data)
        self.assertNotEqual(dag.jobs[0].work.fingerprint, new.jobs[0].work.fingerprint)
        self.assertIn("paths", new.jobs[0].work.parts)
        self.assertNotEqual(plan(new, state_at(moved))[0].status, Status.SKIP)


if __name__ == "__main__":
    unittest.main()
