import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from spit_bash.cause import Cause, Why
from spit_bash.load import load_dag
from spit_bash import files
from spit_bash.plan import AdoptCause, AdoptStatus, Status, adopt, plan
from support import FIXTURE, artifact, job_entry, read, run, sample, state_at, UPPER


class PlanTests(unittest.TestCase):
    def test_missing_source_blocks(self):
        with tempfile.TemporaryDirectory() as directory:
            decisions = plan(read(sample(Path(directory))), state_at(directory))
            self.assertEqual([d.status for d in decisions], [Status.BLOCKED, Status.BLOCKED])
            self.assertEqual(decisions[0].cause.why, Why.MISSING_EXTERNAL)

    def test_plan_reads_each_file_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "source.txt").write_text("hello")
            dag = read(sample(root))
            run(dag, state_at(root))
            with mock.patch.object(files, "stamp", wraps=files.stamp) as stamp:
                self.assertEqual([d.status for d in plan(dag, state_at(root))], [Status.SKIP, Status.SKIP])
            read_paths = [call.args[0].relative_to(root).as_posix() for call in stamp.call_args_list]
            self.assertEqual(sorted(read_paths), ["final.txt", "source.txt", "work/upper.txt"])

    def test_fixture_without_inputs_is_blocked(self):
        with tempfile.TemporaryDirectory() as directory, FIXTURE.open(encoding="utf-8") as source:
            dag = load_dag(source, directory)
            self.assertEqual([d.status for d in plan(dag, state_at(directory))], [Status.BLOCKED] * 5)

    def test_job_without_command_requires_existing_current_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("source")
            data = sample(root)
            data["jobs"] = [data["jobs"][0]]
            data["jobs"][0]["dependents"] = []
            data["jobs"][0]["commands"] = []
            dag = read(data)
            state = state_at(root)
            self.assertEqual(plan(dag, state)[0].status, Status.BLOCKED)
            output = root / "work/upper.txt"
            output.parent.mkdir()
            output.write_text("prepared")
            self.assertEqual(plan(dag, state)[0].status, Status.SKIP)
            source = root / "source.txt"
            source.write_text("new source")
            newer = output.stat().st_mtime_ns + 1_000_000_000
            os.utime(source, ns=(newer, newer))
            self.assertEqual(plan(dag, state)[0].status, Status.BLOCKED)

    def test_missing_program_blocks_its_job_and_dependents_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("hello")
            data = sample(root)
            data["jobs"].append(job_entry(3, ["source.txt"], "other.txt", UPPER))
            data["executables"] = ["spit-bash-no-such-program"]
            data["jobs"][0]["commands"][0]["command"][0] = ["spit-bash-no-such-program"]
            data["jobs"][1]["verify"] = [[["bin/missing-check"]]]
            decisions = plan(read(data), state_at(root))
            self.assertEqual([d.status for d in decisions], [Status.BLOCKED, Status.BLOCKED, Status.RUN])
            self.assertEqual(decisions[0].cause, Cause(Why.PROGRAM_NOT_FOUND, ("spit-bash-no-such-program",)))
            self.assertEqual(decisions[1].cause, Cause(Why.DEPENDENCY_BLOCKED, jobs=(1,)))

    def _with_here(self, root, where, script="missing.py"):
        (root / "source.txt").write_text("hello")
        folder = root / "pipeline"
        folder.mkdir(exist_ok=True)
        data = sample(root)
        data["pipeline_folder"] = str(folder)
        use = [[{"here": ""}, f"/{script}"], [{"path": "source.txt"}]]
        job = data["jobs"][0]
        if where == "command":
            job["commands"][0]["command"] = [["python3"], *use]
        elif where == "verify":
            job["verify"] = [[["python3"], *use]]
        else:
            data["jobs"][0]["checks"] = [{"when": "after", "check": "x", "port": "output", "path": "work/upper.txt",
                                          "command": [["python3"], *use]}]
        return data, folder / script

    def test_a_missing_here_script_blocks_its_job_and_dependents_only(self):
        for where in ("command", "verify"):
            with self.subTest(where), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                data, script = self._with_here(root, where)
                data["jobs"].append(job_entry(3, ["source.txt"], "other.txt", UPPER))
                decisions = plan(read(data), state_at(root))
                self.assertEqual([d.status for d in decisions], [Status.BLOCKED, Status.BLOCKED, Status.RUN])
                self.assertEqual(decisions[0].cause, Cause(Why.HERE_NOT_FOUND, (str(script),)))
                self.assertEqual(decisions[1].cause, Cause(Why.DEPENDENCY_BLOCKED, jobs=(1,)))

    def test_a_missing_here_script_in_a_check_blocks_its_job(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            data, script = self._with_here(root, "check")
            decisions = plan(read(data), state_at(root))
            self.assertEqual(decisions[0].cause, Cause(Why.HERE_NOT_FOUND, (str(script),)))

    def test_a_present_here_script_does_not_block(self):
        for where in ("command", "verify", "check"):
            with self.subTest(where), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                data, script = self._with_here(root, where)
                script.write_text("print()\n")
                self.assertEqual([d.status for d in plan(read(data), state_at(root))], [Status.RUN, Status.RUN])

    def test_a_here_folder_needs_only_the_folder(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            data, script = self._with_here(root, "command")
            data["jobs"][0]["commands"][0]["command"] = [["python3"], [{"here": ""}], [{"path": "source.txt"}]]
            self.assertEqual([d.status for d in plan(read(data), state_at(root))], [Status.RUN, Status.RUN])

    def test_program_named_by_a_path_is_found_from_the_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("hello")
            data = sample(root)
            data["jobs"][1]["verify"] = [[["bin/check"]]]
            self.assertEqual(plan(read(data), state_at(root))[1].cause, Cause(Why.PROGRAM_NOT_FOUND, ("bin/check",)))
            (root / "bin").mkdir()
            (root / "bin/check").write_text("#!/bin/sh\n")
            (root / "bin/check").chmod(0o755)
            self.assertEqual([d.status for d in plan(read(data), state_at(root))], [Status.RUN, Status.RUN])

    def test_state_survives_moving_the_dataset(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first"
            first.mkdir()
            (first / "source.txt").write_text("hello")
            data = sample(None)
            run(read(data, first), state_at(first))
            moved = Path(directory) / "moved"
            first.rename(moved)
            decisions = plan(read(data, moved), state_at(moved))
            self.assertEqual([d.status for d in decisions], [Status.SKIP, Status.SKIP])

    def test_adopt_records_existing_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("hello")
            (root / "work").mkdir()
            (root / "work/upper.txt").write_text("made elsewhere")
            dag = read(sample(root))
            state = state_at(root)
            self.assertEqual([d.cause for d in plan(dag, state)], [Cause(Why.NO_RECORD), Cause(Why.OUTPUT_MISSING)])
            adoptions = adopt(dag, state)
            state.close()
            self.assertEqual([a.status for a in adoptions], [AdoptStatus.ADOPTED, AdoptStatus.LEFT])
            self.assertEqual(adoptions[1].cause, AdoptCause.OUTPUT_MISSING)
            decisions = plan(dag, state_at(root))
            self.assertEqual([(d.status, d.cause.why) for d in decisions], [(Status.SKIP, Why.CURRENT), (Status.RUN, Why.OUTPUT_MISSING)])


if __name__ == "__main__":
    unittest.main()
