"""`join` and `list` parts: a `many` input's paths as one argument, or in a file the runner writes for the job."""

import asyncio
import json
import shutil
import tempfile
import unittest
from pathlib import Path

from spit_bash.execute.options import Options
from spit_bash.execute.scheduler import execute
from spit_bash.model import DagError
from spit_bash.plan import Status, plan
from support import command, document, error, job_entry, new_run, read, run, state_at

# Writes its arguments but the last, as JSON, to the file the last names.
ARGUMENTS = "import json, sys; from pathlib import Path; Path(sys.argv[-1]).write_text(json.dumps(sys.argv[1:-1]))"


class CollectionTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root, True)
        for name in ["two words.txt", "10.txt"]:
            (self.root / name).write_text(name)

    def data(self, part, script=ARGUMENTS):
        item = job_entry(1, ["two words.txt", "10.txt"], "out.txt", script)
        item["commands"][0]["command"] = command(script) + [["--in=", part], [{"path": "out.txt"}]]
        return document(self.root, [item], ["two words.txt", "10.txt"])

    def arguments(self):
        return json.loads((self.root / "out.txt").read_text())

    def test_a_join_is_one_argument_with_its_spaces_kept(self):
        _, results = run(read(self.data({"join": ["two words.txt", "10.txt"], "separator": ","})), state_at(self.root))
        self.assertEqual([error(result) for result in results], [None])
        self.assertEqual(self.arguments(), ["--in=two words.txt,10.txt"])

    def test_an_empty_or_one_member_join_is_still_one_argument(self):
        for members in [[], ["two words.txt"]]:
            for separator in ["", ",", "\" "]:
                dag = read(self.data({"join": members, "separator": separator}))
                _, results = run(dag, state_at(self.root), force=True)
                self.assertEqual([error(result) for result in results], [None])
                self.assertEqual(self.arguments(), ["--in=" + separator.join(members)])

    def test_a_list_is_shared_with_verify_and_removed_after_the_job(self):
        script = ("import json, sys; from pathlib import Path; listed = Path(sys.argv[1].removeprefix('--in='));"
                  " Path(sys.argv[-1]).write_text(json.dumps([str(listed), listed.read_text()]))")
        part = {"list": ["two words.txt", "10.txt"]}
        data = self.data(part, script)
        data["jobs"][0]["verify"] = [[["test"], ["-f"], [part]]]
        dag = read(data)
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None])
        listed, contents = self.arguments()
        self.assertEqual(contents, "".join(f"{self.root / path}\n" for path in part["list"]))
        self.assertFalse(Path(listed).exists())
        # The list's file is elsewhere on each run, and is no part of the work.
        self.assertEqual(dag.jobs[0].work, read(data).jobs[0].work)
        self.assertEqual([decision.status for decision in plan(dag, state_at(self.root))], [Status.SKIP])

    def test_a_list_is_removed_when_its_command_fails(self):
        script = ("import sys; from pathlib import Path;"
                  " Path('seen.txt').write_text(sys.argv[1].removeprefix('--in=')); raise SystemExit(3)")
        _, results = run(read(self.data({"list": ["two words.txt"]}, script)), state_at(self.root))
        self.assertIsNotNone(error(results[0]))
        self.assertFalse(Path((self.root / "seen.txt").read_text()).exists())

    def test_a_list_is_removed_when_its_job_is_cancelled(self):
        script = ("import sys, time; from pathlib import Path;"
                  " Path('seen.txt').write_text(sys.argv[1].removeprefix('--in=')); time.sleep(60)")
        dag = read(self.data({"list": ["two words.txt"]}, script))
        state = state_at(self.root)
        seen = self.root / "seen.txt"

        async def cancel_once_started():
            task = asyncio.ensure_future(execute(dag, plan(dag, state), state,
                                                 Options(new_run(self.root / ".spit-bash/runs"))))
            while not seen.exists() or not seen.read_text():
                await asyncio.sleep(0.01)
            self.assertTrue(Path(seen.read_text()).exists())
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task

        asyncio.run(cancel_once_started())
        state.close()
        self.assertFalse(Path(seen.read_text()).exists())

    def test_a_join_is_checked_under_the_root_the_paths_are_given_from(self):
        other = self.root / "data,with-comma"
        other.mkdir()
        for name in ["two words.txt", "10.txt"]:
            (other / name).write_text("x")
        data = self.data({"join": ["two words.txt", "10.txt"], "separator": ","})
        data["output_root"] = str(self.root / "results")
        with self.assertRaisesRegex(DagError, "holds the join separator ','"):
            read(data, str(other))

    def test_a_collection_names_the_job_s_inputs_and_is_not_in_a_check(self):
        for part in [{"list": ["out.txt"]}, {"join": ["missing"], "separator": ","}]:
            with self.assertRaisesRegex(DagError, "which is not one of its inputs"):
                read(self.data(part))
        data = self.data({"list": ["10.txt"]})
        data["jobs"][0]["commands"][0]["command"].insert(0, [{"list": ["10.txt"]}])
        with self.assertRaisesRegex(DagError, "starts with a `join` or `list` part"):
            read(data)

    def test_an_anchored_job_reruns_after_its_dataset_moves(self):
        data = self.data({"join": ["two words.txt", "10.txt"], "separator": ","})
        data["jobs"][0]["anchored"] = True
        dag = read(data)
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None])
        moved = self.root / "moved"
        shutil.copytree(self.root, moved, ignore=shutil.ignore_patterns("moved"))
        data["root"] = data["output_root"] = str(moved)
        new = read(data)
        self.assertNotEqual(dag.jobs[0].work.parts["paths"], new.jobs[0].work.parts["paths"])
        self.assertEqual([decision.status for decision in plan(new, state_at(moved))], [Status.RUN])


if __name__ == "__main__":
    unittest.main()
