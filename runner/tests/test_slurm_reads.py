"""When a Slurm run reads the queue: every `status_interval` while tasks change, backing off while they do not."""

import asyncio
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from spit_bash.execute.options import Options
from spit_bash.model import Layout
from spit_bash.slurm import behaviour, commands
from spit_bash.slurm.executor import Slurm
from spit_bash.slurm.profile import Profile
from support import document, new_run, read


def row(task: str, state: str) -> commands.Listed:
    return commands.Listed(task, int(task.partition("_")[2]), state, "/elsewhere/other.sh")


class QueueReads(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.layout = Layout(self.root / "runner")
        self.dag = read(document(self.root, [], []))

    def executor(self, profile: Profile, min_job_age: float = 300.0) -> Slurm:
        return Slurm(profile, self.dag, Options(new_run(self.layout.runs), 1, None), self.layout.folder / "slurm",
                     {}, {}, {}, {}, (), {}, commands.Config(min_job_age=min_job_age))

    async def reads(self, executor: Slurm, answers: list[list[commands.Listed]]) -> list[float]:
        gaps = []
        for answer in answers:
            with patch.object(commands, "snapshot", AsyncMock(return_value=answer)):
                await executor._observe()
            gaps.append(executor.gap)
        return gaps

    async def test_the_gap_doubles_while_reads_find_nothing_new_and_resets_when_a_task_changes(self):
        executor = self.executor(Profile(status_interval=10.0, max_status_interval=80.0))
        running = [row("7_0", "RUNNING")]
        ended = [row("7_0", "COMPLETED")]
        gaps = await self.reads(executor, [running, running, running, running, running, ended, ended])
        self.assertEqual(gaps, [10.0, 20.0, 40.0, 80.0, 80.0, 10.0, 20.0])

    async def test_the_gap_stays_under_half_the_sites_minjobage(self):
        executor = self.executor(Profile(status_interval=10.0, max_status_interval=960.0), min_job_age=60.0)
        gaps = await self.reads(executor, [[]] * 5)
        self.assertEqual(max(gaps), 30.0)
        found, _ = behaviour.limits(Profile(status_interval=10.0, max_status_interval=960.0), {"MinJobAge": "60"})
        self.assertIn("every 10s while its tasks change, and at most every 30s while they do not", found.detail)

    def test_a_max_below_the_interval_is_the_interval(self):
        self.assertEqual(self.executor(Profile(status_interval=200.0, max_status_interval=10.0),
                                       min_job_age=1000.0).ceiling, 200.0)

    async def test_a_backed_off_read_waits_for_the_gap_unless_a_job_waits_on_a_read(self):
        executor = self.executor(Profile(status_interval=10.0, max_status_interval=80.0))
        executor.gap = 80.0
        executor.snapshot_at = time.monotonic() - 20.0
        self.assertFalse(executor._snapshot_due())
        # A result whose task Slurm has yet to show ended.
        future = asyncio.get_running_loop().create_future()
        executor.wants["key"] = SimpleNamespace(future=future, result=object(), terminal=None, task="7_0",
                                                not_before=float("-inf"))
        self.assertTrue(executor._snapshot_due())

    async def test_a_missing_task_is_given_up_at_a_read_after_visibility_not_on_a_timer(self):
        executor = self.executor(Profile(status_interval=120.0, visibility=60.0))
        want = SimpleNamespace(future=asyncio.get_running_loop().create_future(), task="7_0", intent="first",
                               terminal=None, missing=None, started=False, ended=None, result=None,
                               identity=None, not_before=float("-inf"))
        executor.wants["key"] = want
        await self.reads(executor, [[]])
        self.assertIsNotNone(want.missing)
        # Longer than `visibility` since the first read, with no read between: the run still waits.
        want.missing -= 100.0
        executor._expire()
        self.assertFalse(want.future.done())
        await self.reads(executor, [[]])
        self.assertTrue(want.future.done())
        self.assertIn("could not be verified", want.future.result().words)


if __name__ == "__main__":
    unittest.main()
