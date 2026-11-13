"""What a DAG's reviews withheld: `plan` and `run` say what waits for a verdict and whether a person can inspect it."""

import json
import tempfile
import unittest
from pathlib import Path

from support import EXCLAIM, UPPER, artifact, job_entry, placed, document
from test_cli import call

FAIL = "import sys; sys.exit(3)"


def target(path, under="output_root", verdict=None, line=None, withholds=("final.txt",)):
    return {"artifact": artifact(path, under), "port": None, "verdict": verdict,
            "reason": "blurred" if verdict else None, "line": line,
            "withholds": list(withholds), "shrinks": []}


def review(file, targets, found=True, verdicts=0, unmatched=()):
    return {"name": "looked_at", "file": None if file is None else str(file), "found": found,
            "verdicts": verdicts, "unmatched": list(unmatched), "targets": targets}


class ReviewTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / "source.txt").write_text("hello")
        self.csv = self.root / "qc/looked_at.csv"

    def saved(self, reviews, first=UPPER):
        """source.txt -> work/upper.txt -> made.txt, where a review holds work/upper.txt from a third job."""
        data = document(self.root, [
            job_entry(1, ["source.txt"], "work/upper.txt", first, dependents=[2]),
            job_entry(2, ["work/upper.txt"], "made.txt", EXCLAIM, depends_on=[1]),
        ], ["source.txt"])
        data["reviews"] = reviews
        path = self.root / "jobs.spitdag"
        path.write_text(json.dumps(placed(data)))
        return str(path)

    def test_a_held_artifact_is_to_make_then_ready_and_withheld_work_is_no_failure(self):
        dag = self.saved([review(self.csv, [target("work/upper.txt")], verdicts=2, unmatched=[4])])
        code, out, _ = call("plan", dag)
        self.assertEqual(code, 0)
        upper = self.root / "work/upper.txt"
        self.assertIn("review `looked_at`: 0 rejected, 1 held; 1 output withheld\n"
                      f"  record verdicts in {self.csv}, which held 2; 1 row matched no artifact (line 4)\n"
                      f"  to inspect once job 1 makes it: {upper}\n", out)
        code, _, err = call("run", dag)
        self.assertEqual(code, 0, err)
        self.assertIn(f"  ready to inspect: {upper}\n", err)
        # Current, with no job to run, it is still listed.
        code, _, err = call("run", dag)
        self.assertIn("0 done", err)
        self.assertIn(f"  ready to inspect: {upper}\n", err)

    def test_an_artifact_whose_job_fails_or_cannot_run_could_not_be_made(self):
        dag = self.saved([review(self.csv, [target("work/upper.txt"), target("made.txt")])], first=FAIL)
        code, _, err = call("run", dag)
        self.assertEqual(code, 1)
        self.assertIn(f"  could not be made, as job 1 failed: {self.root / 'work/upper.txt'}\n", err)
        self.assertIn(f"  could not be made, as job 2 did not run: {self.root / 'made.txt'}\n", err)

    def test_a_rejection_is_listed_with_its_row_and_a_missing_source_is_said(self):
        held = target("gone.txt", under="root", withholds=("a.txt", "b.txt"))
        rejected = target("source.txt", under="root", verdict="fail", line=3)
        dag = self.saved([review(self.csv, [rejected, held], verdicts=1)])
        _, out, _ = call("plan", dag)
        self.assertIn("review `looked_at`: 1 rejected, 1 held; 3 outputs withheld\n", out)
        self.assertIn("  rejected source.txt (line 3): blurred\n", out)
        self.assertIn(f"  missing: {self.root / 'gone.txt'}\n", out)

    def test_the_review_file_is_named_as_it_was_when_the_dag_was_made(self):
        held = [target("work/upper.txt")]
        for reviewed, words in [
            (review(None, held, found=False), "  no file holds its verdicts; name one in the recipe with "
                                              "`review looked_at from FILE`\n"),
            (review(self.csv, held, found=False), f"  record verdicts in {self.csv}, which did not exist "
                                                  "when the DAG was made\n"),
            (review(self.csv, held), f"  record verdicts in {self.csv}, which held none\n"),
        ]:
            with self.subTest(words=words[:30]):
                _, out, _ = call("plan", self.saved([reviewed]))
                self.assertIn(words, out)
        # A review that withheld nothing has nothing to report.
        _, out, _ = call("plan", self.saved([review(self.csv, [])]))
        self.assertNotIn("review `looked_at`", out)

    def test_plan_json_gives_each_held_artifact_its_readiness(self):
        rejected = target("source.txt", under="root", verdict="fail", line=3)
        dag = self.saved([review(self.csv, [rejected, target("work/upper.txt")], verdicts=1)])
        code, out, _ = call("plan", dag, "--json")
        self.assertEqual(code, 0)
        reviews = json.loads(out)["reviews"]
        self.assertEqual([(t["identity"], t["verdict"], t["readiness"], t["job"]) for t in reviews[0]["targets"]],
                         [("source.txt", "fail", None, None), ("work/upper.txt", None, "to make", 1)])
        self.assertEqual(reviews[0]["file"], str(self.csv))


if __name__ == "__main__":
    unittest.main()
