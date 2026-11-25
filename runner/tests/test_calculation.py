import subprocess
import tempfile
import unittest
from pathlib import Path

from spit_bash.model import DagError
from spit_bash.plan import Status
from spit_bash.slurm import result as results
from spit_bash.execute.failure import CalculationFailed
from support import command, document, error, job_entry, read, run, state_at
from test_slurm_job import ENVIRONMENT, job_command, task


class CalculationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.gain = self.root / "gain.txt"
        self.gain.write_text("1.844439727\n")
        (self.root / "reference.txt").write_text("80\n")

    def pipeline(self, expression):
        calc = job_entry(1, ["gain.txt"], "m0.txt", "", dependents=[2])
        calc["inputs"]["exp"] = calc["inputs"].pop("input")
        calc["inputs"]["reference"] = [calc["inputs"]["exp"][0] | {"path": "reference.txt"}]
        calc["commands"] = [{"command": [[{"calculate": expression}]], "stdin": None, "stdout": "m0.txt"}]
        consumer = job_entry(2, ["m0.txt"], "final.txt", "", depends_on=[1])
        writer = "import sys; from pathlib import Path; Path(sys.argv[2]).write_text(sys.argv[1])"
        consumer["commands"][0]["command"] = command(writer) + [
            [{"scalar": "m0.txt", "min": 0, "max": None}],
            [{"path": "final.txt"}],
        ]
        return document(self.root, [calc, consumer], ["gain.txt", "reference.txt"])

    def test_a_calculation_produces_an_ordinary_scalar_and_reruns_when_inputs_change(self):
        dag = read(self.pipeline(["round", ["*", "reference", ["exp", "exp"]], 1]))
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None, None])
        self.assertEqual((self.root / "m0.txt").read_text(), "506.0\n")
        self.assertEqual((self.root / "final.txt").read_text(), "506.0")
        self.gain.write_text("0\n")
        _, results = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None, None])
        self.assertEqual((self.root / "final.txt").read_text(), "80.0")

    def test_identical_rounded_output_keeps_the_consumer_current(self):
        dag = read(self.pipeline(["round", ["*", "reference", ["exp", "exp"]], 1]))
        _, first = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in first], [None, None])
        output = self.root / "m0.txt"
        rounded = output.read_bytes()
        self.assertEqual(rounded, b"506.0\n")

        self.gain.write_text("1.84444\n")
        _, second = run(dag, state_at(self.root))
        self.assertEqual([error(result) for result in second], [None, None])
        self.assertEqual([result.job.id for result in second if result.log is not None], [1])
        self.assertEqual([result.job.id for result in second if result.settled is Status.SKIP], [2])
        self.assertEqual(output.read_bytes(), rounded)

    def test_bad_inputs_and_arithmetic_fail_without_creating_outputs(self):
        dag = read(self.pipeline(["/", "exp", 0]))
        _, results = run(dag, state_at(self.root))
        self.assertIn("division by zero", error(results[0]))
        self.assertFalse((self.root / "m0.txt").exists())
        self.assertFalse((self.root / "final.txt").exists())
        self.gain.write_text("nan")
        _, results = run(dag, state_at(self.root))
        self.assertIn("gain.txt", error(results[0]))
        self.assertIn("holds nan", error(results[0]))

    def test_an_expression_change_reruns_the_calculation_and_its_consumer(self):
        first = read(self.pipeline(["round", 2.5, 0]))
        _, results = run(first, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None, None])
        self.assertEqual((self.root / "final.txt").read_text(), "2")

        changed = read(self.pipeline(["round", 3.5, 0]))
        _, results = run(changed, state_at(self.root))
        self.assertEqual([error(result) for result in results], [None, None])
        self.assertEqual((self.root / "final.txt").read_text(), "4")

    def test_nonfinite_intermediates_and_nonintegral_precision_fail(self):
        for expression, message in [
            (["min", ["*", 1e308, 1e308], 1], "gave a non-finite result"),
            (["round", 1, 1.5], "decimal places must be an integer"),
        ]:
            with self.subTest(expression=expression):
                dag = read(self.pipeline(expression))
                _, results = run(dag, state_at(self.root))
                self.assertIn(message, error(results[0]))
                self.assertFalse((self.root / "m0.txt").exists())

    def test_a_malformed_dag_is_rejected_at_load_time(self):
        for expr in (["exec", "exp"], ["exp", "missing"], True):
            with self.subTest(expr=expr), self.assertRaisesRegex(DagError, "calculation"):
                read(self.pipeline(expr))

    def test_the_dag_rejects_too_few_and_too_many_operands(self):
        for operator, arity in (("exp", 1), ("round", 2)):
            for count in (arity - 1, arity + 1):
                expr = [operator, *([1] * count)]
                with self.subTest(expr=expr), self.assertRaisesRegex(DagError, f"takes {arity} operands"):
                    read(self.pipeline(expr))

    def test_the_slurm_worker_writes_the_calculated_file(self):
        dag = read(self.pipeline(["*", "reference", 2]))
        path, result_path = task(dag, 1)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        self.assertEqual(results.read(result_path).outcome, results.Outcome.MADE)
        self.assertEqual((self.root / "m0.txt").read_text(), "160.0\n")

    def test_the_slurm_worker_reports_calculation_failure(self):
        dag = read(self.pipeline(["sqrt", -1]))
        path, result_path = task(dag, 1)
        self.assertEqual(subprocess.run(job_command(path), env=ENVIRONMENT).returncode, 0)
        result = results.read(result_path)
        self.assertEqual(result.outcome, results.Outcome.FAILED)
        self.assertIsInstance(result.failure, CalculationFailed)
        self.assertFalse((self.root / "m0.txt").exists())
