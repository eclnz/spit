import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from spit_bash.cli import main
from spit_bash.confirm import parse
from support import sample


class Terminal(io.StringIO):
    """Standard input that is a terminal, holding the answers typed at it."""

    def isatty(self):
        return True


def typed(answer, *argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err), \
            mock.patch.object(sys, "stdin", Terminal(answer)):
        code = main(list(argv))
    return code, err.getvalue()


class ConfirmTests(unittest.TestCase):
    """At a terminal, `run` asks before replacing outputs whose jobs took at least --ask-over to make."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / "source.txt").write_text("hello")
        self.dag = self.root / "jobs.spitdag"
        self.dag.write_text(json.dumps(sample(self.root)))
        code, err = typed("", "run", str(self.dag))
        self.assertEqual(code, 0, err)
        self.assertNotIn("Continue?", err)
        (self.root / "source.txt").write_text("changed")

    def test_no_declines_and_runs_nothing(self):
        code, err = typed("n\n", "run", str(self.dag), "--ask-over", "0s")
        self.assertEqual(code, 1)
        self.assertIn("1 job will replace outputs that took under 1s to make:\n"
                      "  op1 (under 1s)\n"
                      "Continue? [y/N] nothing was run\n", err)
        self.assertEqual((self.root / "final.txt").read_text(), "HELLO!")
        summary = sorted((self.root / ".spit-bash" / "runs").glob("*/summary.json"))[-1]
        self.assertEqual(json.loads(summary.read_text())["ended"], "declined")

    def test_yes_at_the_question_runs(self):
        code, err = typed("y\n", "run", str(self.dag), "--ask-over", "0s")
        self.assertEqual(code, 0, err)
        self.assertEqual((self.root / "final.txt").read_text(), "CHANGED!")

    def test_jobs_quicker_than_the_threshold_are_not_asked_about(self):
        code, err = typed("", "run", str(self.dag))
        self.assertEqual(code, 0, err)
        self.assertNotIn("Continue?", err)

    def test_the_yes_flag_and_force_do_not_ask(self):
        code, err = typed("", "run", str(self.dag), "--ask-over", "0s", "--yes")
        self.assertEqual(code, 0, err)
        self.assertNotIn("Continue?", err)
        code, err = typed("", "run", str(self.dag), "--ask-over", "0s", "--force")
        self.assertEqual(code, 0, err)
        self.assertNotIn("Continue?", err)

    def test_ask_over_takes_seconds_minutes_or_hours(self):
        self.assertEqual([parse(text) for text in ("90s", "2m", "1H")], [90, 120, 3600])
        for text in ("2", "m", "1.5m", "-1s"):
            with self.assertRaises(ValueError):
                parse(text)


if __name__ == "__main__":
    unittest.main()
