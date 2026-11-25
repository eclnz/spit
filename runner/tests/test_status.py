import contextlib
import io
import json
import tempfile
import time
import unittest
from pathlib import Path

from spit_bash import cli
from spit_bash.model import Layout
from spit_bash.slurm import site
from spit_bash.slurm.checks import Check, Name, Status
from spit_bash.slurm.profile import Profile
from spit_bash.slurm.site import Report
from support import placed, sample


class StatusTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / "source.txt").write_text("hello")
        self.runner = self.root / ".spit-bash"
        self.data = sample(self.root)
        self.file = self.root / "d.spitdag"
        self.file.write_text(json.dumps(placed(self.data)))

    def main(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = cli.main(list(args))
        return code, out.getvalue(), err.getvalue()

    def status(self, *args):
        return self.main("status", *(args or (str(self.file),)))

    def section(self, out, name):
        lines = out.splitlines()
        start = next(index for index, line in enumerate(lines) if line.startswith(f"{name}: "))
        end = next((index for index in range(start + 1, len(lines)) if not lines[index].startswith("  ")), len(lines))
        return "\n".join(lines[start:end])

    def ran(self):
        code, _, err = self.main("run", str(self.file))
        self.assertEqual(code, 0, err)

    def runs(self):
        return sorted((self.runner / "runs").iterdir())

    def test_a_finished_run_leaves_every_section_ok(self):
        self.ran()
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        self.assertIn("slurm: ok\n  no Slurm reservations", out)
        self.assertIn("state: ok\n  2 job records in state.jsonl", out)
        self.assertIn("finished", self.section(out, "last run"))
        self.assertIn("kept copies: ok\n  no replaced outputs kept", out)
        self.assertIn("leftovers: ok\n  none", out)

    def test_a_fresh_folder_is_ok(self):
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        self.assertIn("no state.jsonl yet", out)
        self.assertIn("last run: ok\n  no run yet", out)

    def test_outputs_with_no_state_need_attention_and_name_adopt(self):
        self.ran()
        (self.runner / "state.jsonl").unlink()
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        self.assertIn("state: needs attention", out)
        self.assertIn("`spit-bash adopt`", out)

    def test_a_state_from_a_newer_runner_needs_attention(self):
        self.runner.mkdir()
        (self.runner / "state.jsonl").write_text('{"version": 99}\n')
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        self.assertIn("state: needs attention", out)
        self.assertIn("newer spit-bash", out)
        self.assertIn("leftovers: ok\n  not checked: the state cannot be read", out)

    def test_an_interrupted_run_needs_attention_with_its_own_words(self):
        self.ran()
        summary = self.runs()[-1] / "summary.json"
        summary.write_text(json.dumps({"ended": "interrupted"}))
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        last = self.section(out, "last run")
        self.assertIn("needs attention", last)
        self.assertIn(": interrupted", last)
        self.assertIn("run.log", last)

    def test_a_run_without_a_summary_is_not_guessed_at(self):
        self.ran()
        (self.runs()[-1] / "summary.json").unlink()
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        self.assertIn("ended without a summary: it is still running, or stopped before it could write one", out)

    def test_held_reviews_and_failures_need_attention(self):
        self.ran()
        summary = self.runs()[-1] / "summary.json"
        summary.write_text(json.dumps({"ended": "finished", "failed": 1, "held": 2}))
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        last = self.section(out, "last run")
        self.assertIn("1 job failed", last)
        self.assertIn("2 jobs held for review", last)
        self.assertIn("`spit-bash plan` names the review", last)
        self.assertIn("`spit-bash log`", last)

    def test_a_restore_or_release_is_not_the_last_run(self):
        self.ran()
        later = self.runner / "runs" / "9999-12-31T23-59-59"
        later.mkdir()
        (later / "summary.json").write_text(json.dumps({"ended": "restored", "from": "x", "jobs": 1}))
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        self.assertIn("finished", self.section(out, "last run"))

    def test_kept_copies_are_listed_but_need_no_attention(self):
        self.ran()
        (self.root / "source.txt").write_text("changed")
        code, _, err = self.main("run", str(self.file), "--yes")
        self.assertEqual(code, 0, err)
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        kept = self.section(out, "kept copies")
        self.assertIn("replaced by 2 jobs", kept)
        self.assertIn("`spit-bash restore --run NAME`", kept)

    def test_leftovers_are_reported_but_need_no_attention(self):
        self.ran()
        data = sample(self.root)
        data["jobs"] = data["jobs"][:1]
        data["jobs"][0]["dependents"] = []
        self.file.write_text(json.dumps(placed(data)))
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        self.assertIn("left over from jobs the pipeline no longer has", self.section(out, "leftovers"))

    def test_without_a_dag_it_reports_the_folder_and_skips_leftovers(self):
        self.ran()
        code, out, _ = self.status("--dir", str(self.runner))
        self.assertEqual(code, 0, out)
        self.assertIn("leftovers: ok\n  not checked: name the DAG or recipe to check them", out)

    def test_a_dag_that_cannot_be_made_still_shows_the_rest(self):
        self.ran()
        broken = self.root / "broken.spitdag"
        broken.write_text("{")
        code, out, _ = self.status(str(broken), "--dir", str(self.runner))
        self.assertEqual(code, 1)
        self.assertIn("state: ok", out)
        self.assertIn("leftovers: needs attention\n  not checked: the DAG could not be made", out)

    def site_report(self, status):
        report = Report(time.time(), site.fingerprint(Profile()), "login", (Check(Name.VISIBILITY, status, "a file took 80s"),))
        site.save(Layout(self.runner), report)

    def test_a_failed_site_check_needs_attention(self):
        self.site_report(Status.FAIL)
        code, out, _ = self.status()
        self.assertEqual(code, 1)
        self.assertIn("slurm: needs attention", out)
        self.assertIn("the latest site check failed: visibility: a file took 80s", out)

    def test_a_passed_site_check_is_ok(self):
        self.site_report(Status.PASS)
        code, out, _ = self.status()
        self.assertEqual(code, 0, out)
        self.assertIn("the latest site check passed", out)

    def test_status_writes_nothing_even_with_a_truncated_state(self):
        self.ran()
        with (self.runner / "state.jsonl").open("a") as stream:
            stream.write('{"key": "x", "rec')
        before = {path: path.read_bytes() for path in self.root.rglob("*") if path.is_file()}
        self.status()
        self.assertEqual({path: path.read_bytes() for path in self.root.rglob("*") if path.is_file()}, before)


if __name__ == "__main__":
    unittest.main()
