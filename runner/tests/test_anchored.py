"""An anchored job's outputs hold their own paths, so where its files are is part of its work."""

import copy
import shutil
import tempfile
import unittest
from pathlib import Path

from spit_bash.plan import Status, plan
from spit_bash.reuse.relocate import MoveCause
from support import read, run, sample, state_at
from test_identity import moved


class AnchoredTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root, True)
        (self.root / "source.txt").write_text("hello")
        self.data = sample(self.root)
        self.data["jobs"][0]["anchored"] = True
        run(read(self.data), state_at(self.root))
        self.relocated = moved(copy.deepcopy(self.data), lambda p: p if p == "source.txt" else "derivatives/" + p)

    def decisions(self, data, move_files=False, root=None):
        return {d.job.id: d for d in plan(read(data, root), state_at(self.root), move_files=move_files)}

    def test_an_unchanged_anchored_job_is_current(self):
        self.assertEqual([d.status for d in self.decisions(self.data).values()], [Status.SKIP, Status.SKIP])

    def test_moved_paths_rerun_an_anchored_job_and_move_only_the_others(self):
        found = self.decisions(self.relocated, move_files=True)
        self.assertEqual(found[1].status, Status.RUN)
        self.assertEqual(found[1].placement.cause, MoveCause.WORK_CHANGED)
        self.assertEqual(found[1].placement.parts, ("paths",))
        self.assertEqual(found[2].status, Status.RUN)

    def test_identical_files_at_new_paths_do_not_make_an_anchored_job_current(self):
        for path in ("work/upper.txt", "final.txt"):
            target = self.root / "derivatives" / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(self.root / path, target)
        found = self.decisions(self.relocated)
        self.assertEqual(found[1].status, Status.RUN)
        self.assertEqual(found[1].cause.parts, ("paths",))

    def test_a_moved_dataset_reruns_an_anchored_job_alone(self):
        moved_root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, moved_root, True)
        shutil.copytree(self.root, moved_root, dirs_exist_ok=True)
        found = {d.job.id: d for d in plan(read(self.data, str(moved_root)), state_at(moved_root))}
        self.assertEqual(found[1].status, Status.RUN)
        self.assertEqual(found[1].cause.parts, ("paths",))


if __name__ == "__main__":
    unittest.main()
