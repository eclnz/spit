"""Reviews applied by the runner to potential work in a saved DAG."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_cli import call


PIPELINE = """\
review looked_at
source raw : Text [sub]
operation copy_raw(input: Text) -> Text @review(looked_at)
operation finish(input: Text) -> Text
operation merge(first: many Text, second: many Text) -> Text
command copy_raw: cp {input} {@output}
command finish: cp {input} {@output}
command merge: printf "%s\u005c\u005cn" @join(first.sub, ",") {first.sub} {second.sub} > {@output}
made = copy_raw(raw)
other = finish(raw)
last = finish(made)
total = merge(made @vary(sub), other @vary(sub))
path raw: data/{sub}.txt
path made: work/{sub}.txt
path other: other/{sub}.txt
path last: last/{sub}.txt
path total: total.txt
"""


class ReviewTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.csv = self.root / "qc.csv"
        (self.root / "data").mkdir()
        for sub in ("01", "02"):
            (self.root / f"data/{sub}.txt").write_text(sub + "\n")
        (self.root / "recipe.spitin").write_text("pipeline pipeline.spit\nroot .\nreview looked_at from qc.csv\n")

    def saved(self, pipeline=PIPELINE):
        spit = os.environ.get("SPIT")
        if not spit:
            self.skipTest("set SPIT to the compiler")
        path = self.root / "jobs.spitdag"
        (self.root / "pipeline.spit").write_text(pipeline)
        process = subprocess.run([spit, "dag", str(self.root / "recipe.spitin"), "-o", str(path)],
                                 capture_output=True, text=True)
        self.assertEqual(process.returncode, 0, process.stderr)
        return str(path)

    def run_saved(self, dag, *flags):
        code, _, err = call("run", dag, "--cores", "2", *flags)
        self.assertEqual(code, 0, err)
        return err

    def test_same_dag_resumes_and_changes_cached_paired_membership(self):
        dag = self.saved()
        code, out, _ = call("plan", dag, "--json")
        self.assertEqual(code, 0, out)
        self.assertEqual(len(json.loads(out)["reviews"][0]["targets"]), 2)
        err = self.run_saved(dag)
        self.assertIn("ready to inspect", err)
        self.assertFalse((self.root / "total.txt").exists())
        self.csv.write_text("sub,verdict,reason\n01,pass,\n02,pass,\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01,02", "01", "02", "01", "02"])
        self.csv.write_text("sub,verdict,reason\n01,pass,\n02,fail,blurred\n")
        self.assertIn("not required", self.run_saved(dag))
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertIn("0 done", self.run_saved(dag))

    def test_bypass_runs_through_missing_or_malformed_verdicts(self):
        dag = self.saved()
        self.csv.write_text("sub,verdict\n01,typo\n")
        code, _, err = call("plan", dag)
        self.assertEqual(code, 2)
        self.assertIn("qc.csv line 2", err)
        self.assertIn("verdicts bypassed", self.run_saved(dag, "--bypass-verdicts"))
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertTrue((self.root / "total.txt").exists())
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)

    def test_automatic_screen_reuses_verdict_and_accepts_a_human_override(self):
        (self.root / "data/03.txt").write_text("03\n")
        recipe = self.root / "recipe.spitin"
        recipe.write_text(recipe.read_text() + "review publish from release.csv\n")
        pipeline = PIPELINE.replace(
            "review looked_at",
            "review looked_at\nreview publish\nscreen usable: grep -qx 01 {@path}",
        ).replace("@review(looked_at)", "@review(looked_at) @screen(usable)").replace(
            "last = finish(made)", "last = finish(made @review(publish))",
        )
        dag = self.saved(pipeline)
        self.assertIn("screen `usable` rejected", self.run_saved(dag))
        record = self.root / "work/made.screens.csv"
        self.assertIn("02,usable,fail,exit status 1", record.read_text())

        def launches():
            logs = (self.root / ".spit-bash/runs").glob("*/jobs/**/*.log")
            return sum(log.read_text().count("$ grep -qx 01 ") for log in logs)

        self.assertEqual(launches(), 3)
        self.assertIn("0 done", self.run_saved(dag))
        self.assertEqual(launches(), 3)
        record.unlink()
        self.assertIn("0 done", self.run_saved(dag))
        self.assertEqual(launches(), 6)
        self.csv.write_text("sub,verdict\n02,pass\n")
        (self.root / "release.csv").write_text("sub,verdict\n02,pass\n")
        report = self.run_saved(dag)
        self.assertIn("ready to inspect", report)
        self.assertIn("screen `usable` rejected", report)
        self.assertFalse((self.root / "last/01.txt").exists())
        self.assertTrue((self.root / "last/02.txt").exists())
        self.assertFalse((self.root / "last/03.txt").exists())
        self.assertIn("02,usable,reviewed", record.read_text())
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01,02", "01", "02", "01", "02"])

    def test_missing_paired_input_only_blocks_a_required_member(self):
        (self.root / "extra").mkdir()
        (self.root / "extra/01.txt").write_text("01\n")
        pipeline = "source extra : Text [sub]\npath extra: extra/{sub}.txt\n" + (
            PIPELINE.replace("other = finish(raw)", "other = finish(extra)"))
        dag = self.saved(pipeline)
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.run_saved(dag)
        self.assertEqual((self.root / "total.txt").read_text().splitlines(), ["01"] * 3)
        self.csv.write_text("sub,verdict\n01,pass\n02,pass\n")
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("nothing was run", err)

    def test_partial_run_does_not_screen_a_blocked_producers_old_output(self):
        pipeline = PIPELINE.replace(
            "review looked_at", "review looked_at\nscreen usable: test -s {@path}",
        ).replace("@review(looked_at)", "@screen(usable)")
        dag = self.saved(pipeline)
        self.run_saved(dag)
        old = self.root / "last/02.txt"
        old.unlink()
        (self.root / "data/02.txt").unlink()
        code, _, err = call("run", dag, "--partial")
        self.assertEqual(code, 1, err)
        self.assertFalse(old.exists(), err)

    def test_rejected_work_does_not_require_its_executable(self):
        pipeline = PIPELINE.replace(
            "review looked_at", "review looked_at\nscreen usable: grep -qx never {@path}",
        ).replace("@review(looked_at)", "@screen(usable)").replace(
            "last = finish(made)", "operation rejected(input: Text) -> Text\n"
            "command rejected: missing-screened-job-command {input} {@output}\n"
            "last = rejected(made)")
        dag = self.saved(pipeline)
        self.assertIn("not required", self.run_saved(dag))
        self.assertFalse((self.root / "last/01.txt").exists())
        self.assertTrue((self.root / "other/01.txt").exists())

    def test_reuse_distinguishes_two_different_members_with_identical_bytes(self):
        for sub in ("01", "02"):
            (self.root / f"data/{sub}.txt").write_text("same\n")
        pipeline = PIPELINE.replace('@join(first.sub, ",") {first.sub} {second.sub}', '{first}')
        dag = self.saved(pipeline)
        for accepted, rejected in (("01", "02"), ("02", "01")):
            self.csv.write_text(f"sub,verdict\n{accepted},pass\n{rejected},fail\n")
            self.run_saved(dag)
            self.assertEqual((self.root / "total.txt").read_text(), f"work/{accepted}.txt\n")

    def test_review_chain_can_run_ahead_in_one_invocation(self):
        def step(index):
            prior = "raw" if index == 1 else f"p{index - 1}"
            return f"p{index} = gated({prior})\npath p{index}: p{index}/{{sub}}.txt\n"

        pipeline = """review looked_at
source raw : Text [sub]
operation gated(input: Text) -> Text @review(looked_at)
command gated: cp {input} {@output}
path raw: data/{sub}.txt
""" + "".join(step(index) for index in range(1, 7))
        dag = self.saved(pipeline)
        self.run_saved(dag, "--bypass-verdicts")
        self.assertTrue((self.root / "p6/02.txt").exists())
        self.csv.write_text("sub,verdict\n01,pass\n02,fail\n")
        self.assertIn("5 job(s) not required", self.run_saved(dag))
        self.assertTrue((self.root / "p6/02.txt").exists())

    def test_one_call_attaches_checks_screens_and_reviews(self):
        pipeline = """review looked_at
check sound: test -s {@path}
screen usable: grep -qx 01 {@path}
source raw : Text [sub]
operation copy_raw(input: Text) -> Text
command copy_raw: cp {input} {@output}
gated = copy_raw(raw @check(sound) @screen(usable) @review(looked_at))
other = copy_raw(raw)
path raw: data/{sub}.txt
path gated: gated/{sub}.txt
path other: other/{sub}.txt
"""
        dag = self.saved(pipeline)
        jobs = json.loads(Path(dag).read_text())["jobs"]
        self.assertEqual([len(job["checks"]) for job in jobs], [1, 1, 0, 0])
        self.run_saved(dag)
        self.assertTrue((self.root / "gated/01.txt").is_file())
        self.assertFalse((self.root / "gated/02.txt").exists())
        self.assertTrue((self.root / "other/02.txt").is_file())
        self.csv.write_text("sub,verdict\n02,fail\n")
        self.run_saved(dag)
        self.assertIn("02,usable,reviewed", (self.root / "raw.screens.csv").read_text())
        self.assertFalse((self.root / "gated/02.txt").exists())
        self.csv.write_text("sub,verdict\n02,pass\n")
        self.run_saved(dag)
        self.assertTrue((self.root / "gated/02.txt").is_file())

    def test_screen_launch_failure_fails_execution(self):
        dag = self.saved(PIPELINE.replace("review looked_at", "review looked_at\nscreen usable: missing-screen-command {@path}")
                         .replace("@review(looked_at)", "@screen(usable)"))
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("screen usable", err)
        self.assertNotIn("screen `usable` rejected", err)
        self.assertFalse((self.root / "total.txt").exists())

    def test_a_signal_is_an_execution_failure_rather_than_a_verdict(self):
        declaration = "screen usable: python3 -c 'import os, signal; os.kill(os.getpid(), signal.SIGTERM)' {@path}"
        dag = self.saved(PIPELINE.replace("review looked_at", f"review looked_at\n{declaration}")
                         .replace("@review(looked_at)", "@screen(usable)"))
        code, _, err = call("run", dag)
        self.assertEqual(code, 1, err)
        self.assertIn("command terminated by signal 15", err)
        self.assertFalse((self.root / "work/made.screens.csv").exists())

    def test_runtime_screens_fail_closed_on_slurm(self):
        dag = self.saved(PIPELINE.replace("review looked_at", "review looked_at\nscreen usable: test -s {@path}")
                         .replace("@review(looked_at)", "@screen(usable)"))
        code, _, err = call("run", dag, "--executor", "slurm")
        self.assertEqual(code, 2)
        self.assertIn("require --executor local", err)
        self.assertFalse((self.root / "work/01.txt").exists())

    def test_bypass_still_runs_artifact_checks(self):
        pipeline = PIPELINE.replace("review looked_at", "review looked_at\ncheck sound: test ! -s {@path}")
        dag = self.saved(pipeline.replace("@review(looked_at)", "@review(looked_at) @check(sound)"))
        code, _, err = call("run", dag, "--bypass-verdicts")
        self.assertEqual(code, 1, err)
        self.assertIn("check sound", err)
        self.assertFalse((self.root / "last/01.txt").exists())


if __name__ == "__main__":
    unittest.main()
