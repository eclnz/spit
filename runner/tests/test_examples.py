"""Run the runnable examples with a real `spit`, so a change to SPIT's DAG format fails here.

The examples are `examples/runnable` at the repository's root, where SPIT's own tests keep them
in step with its syntax. Set SPIT to the binary built from this checkout, or put `spit` on PATH.
"""

import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

SPIT = os.environ.get("SPIT") or shutil.which("spit")
EXAMPLES = Path(__file__).parents[2] / "examples" / "runnable"


def spit_bash(*args, stdin=None):
    # COPYFILE_DISABLE stops macOS `tar` adding `._` files for extended attributes to the archives it makes.
    env = dict(os.environ, PYTHONPATH=str(Path(__file__).parents[1] / "src"), SPIT=SPIT or "spit", COPYFILE_DISABLE="1")
    return subprocess.run([sys.executable, "-m", "spit_bash", *args], input=stdin, env=env,
                          capture_output=True, text=True)


@unittest.skipUnless(SPIT and EXAMPLES.is_dir(), "set SPIT, or put spit on PATH")
class ExampleTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.examples = Path(directory.name) / "examples"
        shutil.copytree(EXAMPLES, self.examples)

    def dag_json(self, recipe):
        return subprocess.run([SPIT, "dag", str(self.examples / recipe), "--json"],
                              check=True, capture_output=True, text=True).stdout

    def test_lines(self):
        dag = self.dag_json("lines/lines.spitin")
        result = spit_bash("run", "-", "-j", "3", stdin=dag)
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.examples / "lines/output/merged"
        self.assertEqual((output / "group=alpha.txt").read_text(), "apple\nbanana\npear\n")
        self.assertEqual((output / "group=beta.txt").read_text(), "yak\nzebra\n")
        result = spit_bash("plan", "-", "--verbose", stdin=self.dag_json("lines/lines.spitin"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.count(" current "), 5, result.stdout)

    def test_steps(self):
        result = spit_bash("run", str(self.examples / "steps/steps.spitin"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.examples / "steps/output/tidied/name=fruit.txt").read_text(), "apple\npear\n")

    def test_runs_a_recipe_or_pipeline_and_chooses_jobs(self):
        lines = self.examples / "lines"
        output = lines / "output/merged"
        result = spit_bash("run", str(lines / "lines.spitin"), "--entity", "group=beta")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("chose 2 of 5 jobs\n", result.stderr)
        self.assertEqual((output / "group=beta.txt").read_text(), "yak\nzebra\n")
        self.assertFalse((output / "group=alpha.txt").exists())

        result = spit_bash("plan", str(lines / "lines.spit"), "--root", str(lines), "--product", "merged", "--verbose")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("[5] current merge group=beta: current", result.stdout)
        self.assertIn("[4] waits   merge group=alpha: waits for", result.stdout)

        inventory = subprocess.run([SPIT, "inputs", str(lines / "lines.spitin")],
                                   check=True, capture_output=True, text=True).stdout
        result = spit_bash("run", str(lines / "lines.spit"), "-", "--root", str(lines), stdin=inventory)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((output / "group=alpha.txt").read_text(), "apple\nbanana\npear\n")

        result = spit_bash("plan", str(lines / "missing.spitin"))
        self.assertEqual(result.returncode, 2)
        self.assertIn("`spit dag` exited with status 1", result.stderr)

    def test_a_declared_script_source_invalidates_its_operation(self):
        folder = self.examples / "declared-script"
        folder.mkdir()
        (folder / "scripts").mkdir()
        (folder / "scripts/transform.py").write_text(
            "import sys\nfrom pathlib import Path\n"
            "print(Path(sys.argv[1]).read_text().upper(), end='')\n"
        )
        (folder / "input.txt").write_text("Hello\n")
        pipeline = folder / "analysis.spit"
        pipeline.write_text(
            "source script : Script .py\n"
            "source lines : Lines .txt\n\n"
            "default path: out/{@product}\n"
            "path script: scripts/transform.py\n"
            "path lines: input.txt\n\n"
            "operation prepare(lines: Lines) -> Lines .txt\n"
            "command prepare: cp {lines} {@output}\n\n"
            "operation process(script: Script, lines: Lines) -> Lines .txt\n"
            "command process: python3 {script} {lines} > {@output}\n\n"
            "prepared = prepare(lines)\n"
            "result = process(script, prepared)\n"
        )

        result = spit_bash("run", str(pipeline), "--root", str(folder))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((folder / "out/result.txt").read_text(), "HELLO\n")
        result = spit_bash("plan", str(pipeline), "--root", str(folder), "--verbose")
        self.assertIn("[1] current prepare: current", result.stdout)
        self.assertIn("[2] current process: current", result.stdout)

        script = folder / "scripts/transform.py"
        script.write_text(script.read_text().replace(".upper()", ".lower()"))
        result = spit_bash("plan", str(pipeline), "--root", str(folder), "--verbose")
        self.assertIn("[1] current prepare: current", result.stdout)
        self.assertIn("[2] ready   process: input changed", result.stdout)
        result = spit_bash("run", str(pipeline), "--root", str(folder))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((folder / "out/result.txt").read_text(), "hello\n")

    def test_verify(self):
        dag = self.examples / "verify/checked_copy.spitdag"
        subprocess.run([SPIT, "dag", str(self.examples / "verify/checked_copy.spitin"), "-o", str(dag)],
                       check=True, capture_output=True)
        result = spit_bash("run", str(dag))
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.examples / "verify/output/copied/part=one.txt"
        self.assertEqual(output.read_text(), "Verified input.\n")

        output.unlink()
        (self.examples / "verify/input/one.txt").write_text("")
        result = spit_bash("run", str(dag))
        self.assertEqual(result.returncode, 1)
        self.assertIn("verify 1 exited with status 1", result.stderr)
        self.assertFalse(output.exists())

    def test_checks(self):
        recipe = str(self.examples / "checks/notes.spitin")
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("2 done, 0 checked, 0 failed", result.stderr)

        # A note without the word fails its output check, though `cp` succeeded.
        (self.examples / "checks/input/tuesday.txt").write_text("Rest.\n")
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 1)
        self.assertIn("[2] failed: check contains(today) on output output output/copied/day=tuesday.txt "
                      "failed: exited with status 1", result.stderr)

        # A changed check is run on the files that exist, without copying again.
        pipeline = self.examples / "checks/notes.spit"
        pipeline.write_text(pipeline.read_text().replace("contains(today)", "contains(for)"))
        result = spit_bash("plan", recipe, "--verbose")
        self.assertIn("[1] check   copy day=monday: checks changed", result.stdout)
        # The failed run put back the earlier output and kept its record, so the job runs for the
        # change that made it fail, not for a record it lost.
        self.assertIn("[2] ready   copy day=tuesday: input changed", result.stdout)
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 1)
        self.assertIn("0 done, 1 checked, 1 failed", result.stderr)

    def test_streams(self):
        recipe = str(self.examples / "streams/streams.spitin")
        # merge declares 64 MiB, more than this run has, so the run is refused.
        result = spit_bash("run", recipe, "-j", "1", "--memory", "32M")
        self.assertEqual(result.returncode, 2)
        self.assertIn("merge needs 64M of memory where the run has 32M", result.stderr)
        # Without enforcement it runs alone, as it also declares two cores and the run has one.
        result = spit_bash("run", recipe, "-j", "1", "--memory", "32M", "--no-enforce-memory")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("needs 2 cores where the run has 1, and 64M of memory where the run has 32M; it will run alone",
                      result.stdout)
        output = self.examples / "streams/output"
        self.assertEqual((output / "loud/plot=garden.txt").read_text(), "APPLES\nPEARS\nPLUMS\n")
        self.assertEqual((output / "merged.txt").read_text(), "APPLES\nCARROTS\nLEEKS\nPEARS\nPLUMS\n")
        self.assertEqual(sorted(path.name for path in (output / "loud").iterdir()),
                         ["plot=allotment.txt", "plot=garden.txt"])
        log = next((self.examples / "streams/.spit-bash/runs").glob("*/jobs/loud/plot=allotment.log")).read_text()
        self.assertIn("$ tr a-z A-Z < input/allotment.txt > output/loud/plot=allotment.txt\n", log)

        # tr prints nothing for an empty list, so its output fails its nonempty check: the failed
        # output is not left behind, and the earlier one is put back.
        (self.examples / "streams/input/garden.txt").write_text("")
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 1)
        self.assertIn("failed: exited with status 1", result.stderr)
        self.assertEqual(sorted(path.name for path in (output / "loud").iterdir()),
                         ["plot=allotment.txt", "plot=garden.txt"])
        self.assertEqual((output / "loud/plot=garden.txt").read_text(), "APPLES\nPEARS\nPLUMS\n")

    def test_numbers(self):
        recipe = str(self.examples / "numbers/numbers.spitin")
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.examples / "numbers/output/numbered"
        self.assertEqual((output / "plot=garden.txt").read_text(), "1/3: apples\n2/3: pears\n3/3: plums\n")
        self.assertEqual((output / "plot=allotment.txt").read_text(), "1/2: carrots\n2/2: leeks\n")
        log = next((self.examples / "numbers/.spit-bash/runs").glob("*/jobs/numbered/plot=garden.log")).read_text()
        self.assertIn("spit-bash: output/size/plot=garden.txt holds 3\n", log)
        self.assertIn("$ awk -v n=3 ", log)

        # An empty list counts 0, under the min of 1: the job fails before awk starts.
        (self.examples / "numbers/input/garden.txt").write_text("")
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 1)
        self.assertIn("output/size/plot=garden.txt, read by `@scalar`, holds 0, which is under the min of 1",
                      result.stderr)

    def test_review(self):
        folder = self.examples / "review"
        recipe = str(folder / "review.spitin")
        tidied = folder / "output/tidied"
        result = spit_bash("plan", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("review `looked_at`: 1 rejected, 1 held; 2 outputs withheld, 1 output made without them\n",
                      result.stdout)
        self.assertIn(f"  to inspect once job 1 makes it: {tidied / 'plot=allotment.txt'}\n", result.stdout)

        # The tidy jobs run for a person to look at; only the garden passed, so only it is counted.
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((folder / "output/total.txt").read_text().split(), ["2"])
        self.assertIn(f"  record verdicts in {folder / 'qc/looked_at.csv'}, which held 2\n", result.stderr)
        self.assertIn("  rejected tidied[plot=orchard] (line 3): figs are not plums\n", result.stderr)
        self.assertIn(f"  ready to inspect: {tidied / 'plot=allotment.txt'}\n", result.stderr)

        # A saved DAG keeps the verdicts it was made with, and a DAG made again applies the new ones.
        saved = folder / "review.spitdag"
        subprocess.run([SPIT, "dag", recipe, "-o", str(saved)], check=True, capture_output=True)
        with open(folder / "qc/looked_at.csv", "a") as verdicts:
            verdicts.write("allotment,pass,\n")
        result = spit_bash("plan", str(saved))
        self.assertIn("1 rejected, 1 held", result.stdout)
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("review `looked_at`: 1 rejected, 0 held; 1 output withheld, 1 output made without them\n",
                      result.stderr)
        self.assertEqual(sorted((folder / "output/total.txt").read_text().split()), ["2", "2"])

    def test_a_review_file_that_cannot_be_used_stops_a_recipe_before_any_job_runs(self):
        folder = self.examples / "review"
        recipe = str(folder / "review.spitin")
        verdicts = folder / "qc/looked_at.csv"
        saved = folder / "review.spitdag"
        subprocess.run([SPIT, "dag", recipe, "-o", str(saved)], check=True, capture_output=True)
        for held, problem in [
            (b"plot,verdict,reason\ngarden,maybe,\n", "qc/looked_at.csv line 2: verdict `maybe` must be `pass` or `fail`"),
            (b"", "qc/looked_at.csv: expected a header row"),
            (b"plot,verdict\ngarden,\xff\n", "cannot read `qc/looked_at.csv` for `review looked_at`"),
        ]:
            with self.subTest(problem=problem):
                verdicts.write_bytes(held)
                for command in ("plan", "run"):
                    result = spit_bash(command, recipe)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertIn(problem, result.stderr)
                    self.assertIn("`spit dag` exited with status 1", result.stderr)
                    self.assertFalse((folder / "output").exists())
        # A saved DAG holds the verdicts it was made with, whatever becomes of the file.
        result = spit_bash("plan", str(saved))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("review `looked_at`: 1 rejected, 1 held", result.stdout)
        verdicts.unlink()
        result = spit_bash("run", str(saved))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((folder / "output/total.txt").read_text().split(), ["2"])
        self.assertIn("review `looked_at`: 1 rejected, 1 held", result.stderr)

    def test_sequence(self):
        result = spit_bash("run", str(self.examples / "sequence/sequence.spitin"))
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.examples / "sequence/output"
        self.assertEqual((output / "sorted/plot=garden.txt").read_text(), "bean\nbean\ncarrot\ncarrot\ncarrot\npea\n")
        counts = (output / "counts/plot=garden.txt").read_text().split("\n")
        self.assertEqual([line.split() for line in counts if line], [["2", "bean"], ["3", "carrot"], ["1", "pea"]])
        log = next((self.examples / "sequence/.spit-bash/runs").glob("*/jobs/counts/plot=allotment.log")).read_text()
        commands = [line for line in log.splitlines() if line.startswith("$ ")]
        self.assertEqual(commands, [
            "$ sort -o output/sorted/plot=allotment.txt input/allotment.txt",
            "$ uniq -c output/sorted/plot=allotment.txt > output/counts/plot=allotment.txt",
        ])
        result = spit_bash("plan", str(self.examples / "sequence/sequence.spitin"), "--verbose")
        self.assertEqual(result.stdout.count(" current "), 2, result.stdout)

    def test_folders(self):
        recipe = str(self.examples / "folders/albums.spitin")
        result = spit_bash("run", recipe, "-j", "2")
        self.assertEqual(result.returncode, 0, result.stderr)
        album = self.examples / "folders/output/album/trip=alpha"
        self.assertEqual((album / "day1/morning.txt").read_text(), "harbour\n")
        with tarfile.open(self.examples / "folders/output/archive/trip=alpha.tar") as archive:
            self.assertEqual(sorted(archive.getnames()), [".", "./day1", "./day1/morning.txt", "./evening.txt"])

        (self.examples / "folders/input/alpha/day1/morning.txt").write_text("harbour at dawn\n")
        result = spit_bash("plan", recipe, "--verbose")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("[1] ready   copy trip=alpha: input changed", result.stdout)
        self.assertIn("[2] current copy trip=beta: current", result.stdout)
        result = spit_bash("run", recipe)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((album / "day1/morning.txt").read_text(), "harbour at dawn\n")


if __name__ == "__main__":
    unittest.main()
