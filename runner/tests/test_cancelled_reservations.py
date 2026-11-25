import json
import tempfile
import unittest
from pathlib import Path

from spit_bash.slurm.dispatch import Dispatches
from spit_bash.slurm.reconcile import Task


def queued(array: str, index: int, state: str = "PENDING") -> Task:
    return Task(f"{array}_{index}", "a", index, state)


class CancelledReservationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "a.dispatch").write_text('["17", [0, 1, 2, 3]]')
        self.records = Dispatches(self.root)

    def left(self) -> set[str]:
        return set(Dispatches(self.root).tasks())

    def test_a_cancelled_task_the_queue_no_longer_lists_has_ended(self):
        self.records.cancel(["17_0", "17_1"])
        self.records.reconcile((queued("17", 2), queued("17", 3)))
        self.assertEqual(self.left(), {"17_2", "17_3"})

    def test_a_task_never_cancelled_stays_until_it_is_proved_to_have_ended(self):
        self.records.reconcile((queued("17", 0),))
        self.assertEqual(self.left(), {"17_0", "17_1", "17_2", "17_3"})

    def test_a_cancelled_task_still_in_the_queue_stays(self):
        self.records.cancel(["17_0", "17_1"])
        self.records.reconcile((queued("17", 0, "RUNNING"), queued("17", 1, "COMPLETING")))
        self.assertEqual(self.left(), {"17_0", "17_1", "17_2", "17_3"})

    def test_a_read_of_chosen_tasks_proves_nothing_about_the_others(self):
        self.records.cancel(["17_0", "17_1"])
        self.records.reconcile((queued("17", 2),), complete=False)
        self.assertEqual(self.left(), {"17_0", "17_1", "17_2", "17_3"})

    def test_the_reservation_goes_when_every_task_is_cancelled_and_gone(self):
        self.records.cancel([f"17_{index}" for index in range(4)])
        self.records.reconcile(())
        self.assertEqual(list(self.root.glob("*.dispatch")), [])

    def test_the_cancelled_list_keeps_only_tasks_still_outstanding(self):
        self.records.cancel(["17_0", "17_2"])
        self.records.reconcile((queued("17", 1), queued("17", 3), queued("17", 2, "RUNNING")))
        self.assertEqual(json.loads((self.root / "a.dispatch").read_text()), ["17", [1, 2, 3], [2]])


if __name__ == "__main__":
    unittest.main()
