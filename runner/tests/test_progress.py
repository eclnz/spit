import tempfile
import unittest
from dataclasses import replace
from pathlib import Path
from unittest import mock

from spit_bash import progress as timing
from spit_bash.state.store import Store
from spit_bash.progress import Progress, duration
from support import job, read, run, sample, state_at


def jobs(*operations):
    return [job(n, operation=op, product=op) for n, op in enumerate(operations, 1)]


class ProgressTests(unittest.TestCase):
    def test_a_job_with_no_time_takes_its_operations_mean(self):
        a1, a2, b1 = jobs("a", "a", "b")
        progress = Progress.of([a1, a2, b1], (a1, a2, b1), {}, cores=1)
        self.assertIsNone(progress.remaining())
        self.assertEqual(progress.report(), "3 left")
        with mock.patch.object(timing.time, "monotonic", return_value=0.0):
            progress.started(a1)
        progress.finished(a1, 10.0)
        self.assertIsNone(progress.remaining())
        with mock.patch.object(timing.time, "monotonic", return_value=0.0):
            progress.started(b1)
        progress.finished(b1, 4.0)
        self.assertEqual(progress.remaining(), 10.0)
        self.assertEqual(progress.report(), "1 left, about 10s")

    def test_times_from_an_earlier_run_give_an_estimate_at_once(self):
        a1, a2, a3, a4 = jobs("a", "a", "a", "a")
        progress = Progress.of([a1, a2, a3], (a1, a2, a3, a4), {1: 60.0, 4: 120.0}, cores=2)
        self.assertEqual(progress.remaining(), (60 + 90 + 90) / 2)

    def test_a_running_job_counts_for_what_it_has_left(self):
        a1, a2, a3 = jobs("a", "a", "a")
        progress = Progress.of([a1, a2, a3], (a1, a2, a3), {1: 100.0, 2: 100.0, 3: 100.0}, cores=2)
        with mock.patch.object(timing.time, "monotonic", return_value=1000.0):
            progress.started(a1)
        with mock.patch.object(timing.time, "monotonic", return_value=1030.0):
            self.assertEqual(progress.remaining(), (70 + 100 + 100) / 2)

    def test_a_chain_is_decided_by_its_length_not_the_cores(self):
        chain = [replace(job, depends_on=(job.id - 1,) if job.id > 1 else ()) for job in jobs("a", "b", "c", "d")]
        progress = Progress.of(chain, tuple(chain), {1: 10.0, 2: 20.0, 3: 30.0, 4: 40.0}, cores=16)
        self.assertEqual(progress.remaining(), 100.0)
        with mock.patch.object(timing.time, "monotonic", return_value=1000.0):
            progress.started(chain[0])
        with mock.patch.object(timing.time, "monotonic", return_value=1006.0):
            self.assertEqual(progress.remaining(), 4 + 20 + 30 + 40)

    def test_a_wide_dag_is_decided_by_its_work_shared_among_the_cores(self):
        wide = jobs(*"a" * 8, "group")
        wide[-1] = replace(wide[-1], depends_on=tuple(range(1, 9)))
        progress = Progress.of(wide, tuple(wide), {job.id: 10.0 for job in wide}, cores=2)
        self.assertEqual(progress.remaining(), 45.0)

    def test_work_is_counted_in_core_seconds_shared_among_the_cores(self):
        pair = [replace(job, cores=2) for job in jobs("a", "a")]
        self.assertEqual(Progress.of(pair, tuple(pair), {1: 10.0, 2: 10.0}, cores=4).remaining(), 10.0)
        self.assertEqual(Progress.of(pair, tuple(pair), {1: 10.0, 2: 10.0}, cores=2).remaining(), 20.0)
        big = [replace(job, cores=16) for job in jobs("a", "a")]
        self.assertEqual(Progress.of(big, tuple(big), {1: 10.0, 2: 10.0}, cores=4).remaining(), 20.0)
        wide = [replace(job, cores=3 if job.id == 2 else 1) for job in jobs("a", "a")]
        progress = Progress.of(wide, tuple(wide), {1: 10.0}, cores=4)
        self.assertEqual(progress.remaining(), 10.0)
        with mock.patch.object(timing.time, "monotonic", return_value=0.0):
            progress.started(wide[0])
        progress.finished(wide[0], 10.0)
        with mock.patch.object(timing.time, "monotonic", return_value=0.0):
            progress.started(wide[1])
        with mock.patch.object(timing.time, "monotonic", return_value=4.0):
            self.assertEqual(progress.remaining(), 6.0)

    def test_a_group_job_after_every_subject_lengthens_the_estimate(self):
        wide = jobs("subject", "subject", "template")
        wide[-1] = replace(wide[-1], depends_on=(1, 2))
        progress = Progress.of(wide, tuple(wide), {1: 10.0, 2: 10.0, 3: 50.0}, cores=4)
        self.assertEqual(progress.remaining(), 60.0)

    def test_the_longest_path_is_worked_out_again_as_estimates_change(self):
        a, b2, b3, b4, b5 = jobs("a", "b", "b", "b", "b")
        b2, b3, b4 = (replace(b2, depends_on=(1,)), replace(b3, depends_on=(2,)), replace(b4, depends_on=(3,)))
        progress = Progress.of([a, b2, b3, b4, b5], (a, b2, b3, b4, b5), {1: 10.0}, cores=4)
        self.assertIsNone(progress.remaining())
        with mock.patch.object(timing.time, "monotonic", return_value=1000.0):
            progress.started(a)
            progress.finished(a, 10.0)
            progress.started(b2)
            progress.started(b5)
            progress.finished(b2, 30.0)
            self.assertEqual(progress.remaining(), 60.0)
            progress.finished(b5, 90.0)
        with mock.patch.object(timing.time, "monotonic", return_value=1001.0):
            self.assertEqual(progress.remaining(), 60.0)
        with mock.patch.object(timing.time, "monotonic", return_value=1000.0 + timing.REFRESH_SECONDS):
            self.assertEqual(progress.remaining(), 120.0)

    def test_a_dropped_job_is_no_longer_left(self):
        a1, a2 = jobs("a", "a")
        progress = Progress.of([a1, a2], (a1, a2), {1: 5.0, 2: 7.0}, cores=1)
        progress.dropped(a2)
        self.assertEqual((progress.left, progress.remaining()), (1, 5.0))

    def test_an_estimate_under_a_second_says_so(self):
        (a1,) = jobs("a")
        progress = Progress.of([a1], (a1,), {1: 0.2}, cores=1)
        self.assertEqual(progress.report(), "1 left, under 1s")

    def test_duration(self):
        self.assertEqual([duration(s) for s in (0.4, 45, 720, 3 * 3600 + 5 * 60)], ["under 1s", "45s", "12m", "3h05m"])

    def test_a_run_records_each_jobs_time(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("hello")
            dag = read(sample(root))
            run(dag, state_at(root))
            records = Store(root / ".spit-bash" / "state.jsonl").jobs
            for job in dag.jobs:
                self.assertGreater(records[job.work.key].seconds, 0)


if __name__ == "__main__":
    unittest.main()
