"""Fake scheduler teardown waits for writers independently of cancellation acknowledgements."""

import json
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

from fakeslurm.cleanup import shutdown

FAKE = Path(__file__).parent / "fakeslurm"


class ShutdownTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.fake = self.root / "slurm"
        self.fake.mkdir()
        self.addCleanup(shutdown, self.fake)
        self.env = dict(os.environ, FAKE_SLURM_DIR=str(self.fake),
                        FAKE_SLURM_HOLD="1", FAKE_SLURM_EXIT_DELAY="0.25")

    def submit(self, *, held=False, env=None):
        script = self.root / "job.sh"
        script.write_text("#!/bin/bash\nexec sleep 60\n")
        args = [str(FAKE / "sbatch"), "--parsable", f"--output={self.root / '%A_%a.out'}"]
        done = subprocess.run([*args, *(["--hold"] if held else []), str(script)], env=dict(self.env, **(env or {})),
                              capture_output=True, text=True, check=True, timeout=10)
        return done.stdout.strip()

    def test_cancel_acknowledgement_is_followed_by_the_workers_final_write(self):
        array = self.submit()
        subprocess.run([str(FAKE / "scancel"), array], env=self.env, check=True, timeout=10)
        shutdown(self.fake)
        self.assertTrue((self.fake / f"{array}.finished").exists())
        self.directory.cleanup()
        self.assertFalse(self.root.exists())

    def test_shutdown_reaps_a_running_job_even_if_scancel_is_disabled(self):
        array = self.submit(env={"FAKE_SLURM_HOLD": "", "FAKE_SLURM_SCANCEL_NOOP": "1"})
        task_path = self.fake / "tasks" / f"{array}_0.json"
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if json.loads(task_path.read_text())["pid"] is not None:
                break
            time.sleep(0.02)
        pid = json.loads(task_path.read_text())["pid"]
        self.assertIsNotNone(pid, "the fake job must start before teardown")
        shutdown(self.fake)
        self.assertTrue((self.fake / f"{array}.finished").exists())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_held_arrays_have_no_worker_to_join(self):
        array = self.submit(held=True)
        shutdown(self.fake)
        self.assertFalse((self.fake / f"{array}.finished").exists())

    def test_a_missing_worker_acknowledgement_fails_teardown_loudly(self):
        path = self.fake / "1.env"
        path.write_text('{}')
        try:
            with self.assertRaisesRegex(RuntimeError, "workers did not finish"):
                shutdown(self.fake, timeout=0)
        finally:
            path.unlink()


if __name__ == "__main__":
    unittest.main()
