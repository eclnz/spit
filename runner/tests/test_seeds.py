"""An output folder that `seed_folders` starts as a copy of an input folder, afresh on each attempt."""

import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from support import artifact, document, error, read, run, state_at

# Appends a line to `notes.txt` in the folder it is given, and fails when the folder holds `fail`.
APPEND = ("import sys; from pathlib import Path; folder = Path(sys.argv[1]);"
          " (folder / 'notes.txt').write_text((folder / 'notes.txt').read_text() + sys.argv[2] + '\\n');"
          " raise SystemExit(3 if (folder / 'fail').exists() else 0)")


def folder(path):
    return dict(artifact(path), kind="folder")


def seeded(id, output, word):
    """A job that starts `output` from `prepared` and appends `word` to its notes."""
    return {
        "id": id, "operation": f"op{id}", "stage": [], "dependents": [], "depends_on": [],
        "inputs": {"input": [folder("prepared")]}, "outputs": {"result": folder(output)},
        "verify": [], "checks": [], "origin": None, "cores": 1, "memory": None,
        "commands": [{"command": [[sys.executable], ["-c"], [APPEND], [{"path": output}], [word]],
                      "stdin": None, "stdout": None}],
        "seed_folders": {"result": "input"},
    }


class SeedTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root, True)
        prepared = self.root / "prepared"
        (prepared / "inner").mkdir(parents=True)
        (prepared / "notes.txt").write_text("prepared\n")
        (prepared / "inner" / "data.txt").write_text("data")
        os.symlink("inner/data.txt", prepared / "data-link")

    def data(self, *jobs):
        data = document(self.root, list(jobs), ["prepared"])
        data["external_inputs"] = [folder("prepared")]
        return data

    def test_each_branch_starts_from_the_input_and_leaves_it_as_it_was(self):
        _, results = run(read(self.data(seeded(1, "soft", "soft"), seeded(2, "hard", "hard"))), state_at(self.root))
        self.assertEqual([error(result) for result in results], [None, None])
        self.assertEqual((self.root / "soft/notes.txt").read_text(), "prepared\nsoft\n")
        self.assertEqual((self.root / "hard/notes.txt").read_text(), "prepared\nhard\n")
        self.assertEqual((self.root / "prepared/notes.txt").read_text(), "prepared\n")
        self.assertEqual(os.readlink(self.root / "soft/data-link"), "inner/data.txt")
        self.assertEqual((self.root / "soft/data-link").read_text(), "data")

    def test_a_retry_starts_again_from_the_input(self):
        data = self.data(seeded(1, "soft", "soft"))
        (self.root / "prepared/fail").write_text("")
        _, results = run(read(data), state_at(self.root), keep_failed=True)
        self.assertIsNotNone(error(results[0]))
        (self.root / "prepared/fail").unlink()
        _, results = run(read(data), state_at(self.root))
        self.assertEqual([error(result) for result in results], [None])
        self.assertEqual((self.root / "soft/notes.txt").read_text(), "prepared\nsoft\n")

    def test_a_link_out_of_the_input_fails_the_job(self):
        os.symlink(self.root / "elsewhere", self.root / "prepared/out-link")
        _, results = run(read(self.data(seeded(1, "soft", "soft"))), state_at(self.root))
        self.assertIn("leads outside the folder it is copied from", error(results[0]))

    def test_seeding_is_part_of_the_work(self):
        data = self.data(seeded(1, "soft", "soft"))
        seeded_work = read(data).jobs[0].work
        del data["jobs"][0]["seed_folders"]
        self.assertNotEqual(seeded_work.fingerprint, read(data).jobs[0].work.fingerprint)
        self.assertIn("seeds", seeded_work.parts)

    def test_a_seed_names_an_output_folder_and_an_input_folder(self):
        for seeds, message in [({"missing": "input"}, "not one of its output folders"),
                               ({"result": "missing"}, "not an input port holding one folder")]:
            data = self.data(seeded(1, "soft", "soft"))
            data["jobs"][0]["seed_folders"] = seeds
            with self.assertRaisesRegex(DagError, message):
                read(data)


if __name__ == "__main__":
    unittest.main()
