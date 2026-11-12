"""A `scalar` part: the number in an input file, read when the job starts and passed as text."""

import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.reuse import identity
from spit_bash.scalars import Read
from spit_bash.schema import SpitJob
from support import command, document, error, job_entry, job_log, read, run, state_at

# Write the arguments after the script's own to the output, the last argument.
ECHO = "import sys; from pathlib import Path; Path(sys.argv[-1]).write_text(' '.join(sys.argv[1:-1]))"


def scalar(path="tr.txt", least=None, greatest=None):
    return {"scalar": path, "min": least, "max": greatest}


def reading(root, *arguments, verify=()):
    """tr.txt -> out.txt, whose command gives `arguments`, then the output's path, to ECHO."""
    item = job_entry(1, ["tr.txt"], "out.txt", ECHO)
    item["commands"][0]["command"] = command(ECHO) + [list(argument) for argument in arguments] + [[{"path": "out.txt"}]]
    item["verify"] = [list(command) for command in verify]
    return document(root, [item], ["tr.txt"])


class ScalarTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.tr = self.root / "tr.txt"
        self.output = self.root / "out.txt"

    def test_the_trimmed_number_is_the_argument_or_text_within_one(self):
        self.tr.write_text("6.234000 \n")
        is_six = [["test"], [scalar()], ["="], ["6.234000"]]
        dag = read(reading(self.root, ["--tr"], [scalar(least=5)], ["--te=", scalar(), "s"], verify=[is_six]))
        self.assertEqual(dag.jobs[0].reads, (Read("tr.txt", 5.0), Read("tr.txt")))
        # A plan shows the read, since the number is not known until the job starts.
        self.assertIn("@scalar(tr.txt, min=5)", dag.jobs[0].commands[0].arguments)
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(r) for r in results], [None])
        self.assertEqual(self.output.read_text(), "--tr 6.234000 --te=6.234000s")
        log = job_log(self.root / ".spit-bash", "out.txt").read_text()
        self.assertIn("spit-bash: tr.txt holds 6.234000\n", log)
        self.assertIn("$ test 6.234000 = 6.234000\n", log)

    def test_a_file_that_holds_no_allowed_number_fails_the_job_before_it_starts(self):
        for held, words in [
            ("4.1\n", "holds 4.1, which is under the min of 5"),
            ("101", "holds 101, which is over the max of 100"),
            ("", "holds nothing, which is not one number"),
            ("6.2 7.1", "holds '6.2 7.1', which is not one number"),
            ("nan", "holds nan, which is not one number"),
            ("1e999", "holds 1e999, which is too large to be a number"),
            ("x" * 5000, "holds more than 4096 bytes, which is not one number"),
        ]:
            with self.subTest(held=held[:10]):
                self.tr.write_text(held)
                dag = read(reading(self.root, [scalar(least=5, greatest=100)], verify=[[["false"]]]))
                _, results = run(dag, state_at(self.root))
                # The verify command would fail; the read fails first.
                self.assertEqual([error(r) for r in results], [f"tr.txt, read by `@scalar`, {words}"])
                self.assertFalse(self.output.exists())

    def test_the_dag_is_checked_for_reads_of_its_inputs(self):
        self.tr.write_text("6")
        not_input = reading(self.root, [scalar("elsewhere.txt")])
        with self.assertRaisesRegex(DagError, "names elsewhere.txt, which is not one of its inputs"):
            read(not_input)
        first = reading(self.root)
        first["jobs"][0]["commands"][0]["command"] = [[scalar()]]
        with self.assertRaisesRegex(DagError, "starts with a `scalar` part, not a program"):
            read(first)

    def test_the_bounds_are_part_of_the_work_and_the_number_is_the_inputs(self):
        def fingerprint(least):
            item = reading(self.root, [scalar(least=least)])["jobs"][0]
            for artifact in [*item["inputs"]["input"], item["outputs"]["output"]]:
                artifact.setdefault("under", "root")
            job = SpitJob.model_validate(item)
            return identity._parts(job, identity.slots(job))["commands"]

        self.assertEqual(fingerprint(5)[0][0][-2], [["scalar", "in0.0", 5.0, None]])
        self.assertNotEqual(fingerprint(5), fingerprint(6))


if __name__ == "__main__":
    unittest.main()
