import shutil
import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.state.record import Record
from spit_bash.state.store import READABLE, VERSION, StateWrite, Store

FIXTURES = Path(__file__).parent / "fixtures"

RECORD = Record(fingerprint="a" * 16, inputs={"in.txt": [1, 2]}, outputs={"out.txt": [3, 4]})


class StateTests(unittest.TestCase):
    def test_each_change_appends_one_line_and_close_compacts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            state = Store(path)
            for n in range(3):
                state.record(f"key{n}", RECORD)
            state.forget("key1")
            self.assertEqual(len(path.read_text().splitlines()), 5)
            self.assertEqual(set(Store(path).jobs), {"key0", "key2"})
            state.close()
            self.assertEqual(len(path.read_text().splitlines()), 3)
            self.assertEqual(Store(path).jobs["key2"], RECORD)

    def test_apply_records_and_forgets_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            state = Store(path)
            state.record("old", RECORD)
            state.apply((StateWrite("new", RECORD), StateWrite("old", None), StateWrite("missing", None)))
            self.assertEqual(state.jobs, {"new": RECORD})
            self.assertEqual(len(path.read_text().splitlines()), 4)
            state.close()
            self.assertEqual(Store(path).jobs, {"new": RECORD})

    def test_ignores_a_partial_last_line(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            state = Store(path)
            state.record("key", RECORD)
            state.close()
            with path.open("a") as stream:
                stream.write('{"key": "other", "rec')
            self.assertEqual(set(Store(path).jobs), {"key"})
            self.assertTrue(path.read_text().endswith("\n"))

    def test_rejects_a_corrupt_line(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            path.write_text('{"version": 9}\nnot json\n{"key": "k", "record": null}\n')
            with self.assertRaisesRegex(DagError, "cannot read state"):
                Store(path)
            path.write_text('{"version": 1, "jobs": {}}\n')
            with self.assertRaisesRegex(DagError, "cannot read state"):
                Store(path)
            path.write_text('{"version": 1}\n')
            with self.assertRaisesRegex(DagError, "no longer reads"):
                Store(path)

    def test_refusals_say_what_to_do_without_telling_the_user_to_delete(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            path.write_text(f'{{"version": {VERSION + 1}}}\n')
            with self.assertRaisesRegex(DagError, "newer spit-bash.*Upgrade"):
                Store(path)
            path.write_text('{"version": 1}\n')
            with self.assertRaisesRegex(DagError, "move the file aside.*every job will then run again") as caught:
                Store(path)
            self.assertNotIn("delete", str(caught.exception))

    def test_reads_a_state_of_an_earlier_version_and_rewrites_it_at_the_current_one(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            shutil.copy(FIXTURES / "state_v8.jsonl", path)
            self.assertEqual(Store(path, write=False).jobs["job"], RECORD)
            self.assertTrue(path.read_text().startswith('{"version":8}'))
            state = Store(path)
            state.record("other", RECORD)
            self.assertEqual(path.read_text().splitlines()[0], f'{{"version":{VERSION}}}')
            state.close()
            self.assertEqual(set(Store(path).jobs), {"job", "other"})

    def test_every_released_version_stays_readable_and_has_a_stored_file(self):
        self.assertIn(VERSION, READABLE)
        self.assertEqual(READABLE, frozenset(range(min(READABLE), VERSION + 1)), "a readable version is missing")
        for version in sorted(READABLE):
            with self.subTest(version=version):
                stored = FIXTURES / f"state_v{version}.jsonl"
                self.assertTrue(stored.exists(), f"add {stored.name}, a state file as that version wrote it")
                self.assertTrue(stored.read_text().startswith(f'{{"version":{version}}}\n'))
                self.assertEqual(Store(stored, write=False).jobs["job"].fingerprint, "a" * 16)

    def test_an_empty_state_gets_its_header_before_the_first_record(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            path.write_text("")
            state = Store(path)
            state.record("key", RECORD)
            self.assertEqual(Store(path).jobs, {"key": RECORD})

    def test_lock_allows_one_holder(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.jsonl"
            first = Store(path)
            first.lock()
            with self.assertRaisesRegex(DagError, "another spit-bash run"):
                Store(path).lock()
            first.close()
            second = Store(path)
            second.lock()
            second.close()


if __name__ == "__main__":
    unittest.main()
