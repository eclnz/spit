import unittest

from spit_bash.slurm.reconcile import Identity, Mode, Task, newest, reconcile, releasable

A = Identity("a", "work-1", Mode.RUN)
A2 = Identity("a", "work-2", Mode.RUN)
A_CHECK = Identity("a", "work-1", Mode.CHECK)
B = Identity("b", "work-1", Mode.RUN)


def tasks(*rows):
    found = {}
    listed = []
    for task_id, identity, state in rows:
        task = Task(task_id, None if identity is None else "i", int(task_id.split("_")[1]), state)
        found[task_id] = identity
        listed.append(task)
    return tuple(listed), lambda task: found[task.id]


class ReconcileTests(unittest.TestCase):
    def test_a_task_doing_the_wanted_work_is_waited_on(self):
        snapshot, identify = tasks(("5_0", A, "RUNNING"))
        decided = reconcile({"a": A}, snapshot, identify, {})
        self.assertEqual(decided.waits["a"].id, "5_0")
        self.assertEqual((decided.cancel, decided.free), ((), frozenset()))

    def test_a_task_of_older_work_is_cancelled_and_its_key_is_not_free_until_it_ends(self):
        snapshot, identify = tasks(("5_0", A, "RUNNING"))
        decided = reconcile({"a": A2}, snapshot, identify, {})
        self.assertEqual([task.id for task in decided.cancel], ["5_0"])
        self.assertNotIn("a", decided.free)
        ended, identify = tasks(("5_0", A, "CANCELLED"))
        self.assertIn("a", reconcile({"a": A2}, ended, identify, {}).free)

    def test_a_task_of_work_the_plan_does_not_do_is_cancelled(self):
        snapshot, identify = tasks(("5_0", A, "PENDING"), ("6_0", B, "PENDING"))
        decided = reconcile({"b": B}, snapshot, identify, {})
        self.assertEqual([task.id for task in decided.cancel], ["5_0"])
        self.assertEqual(decided.waits["b"].id, "6_0")

    def test_a_task_running_a_job_planned_only_for_its_checks_is_cancelled(self):
        snapshot, identify = tasks(("5_0", A, "RUNNING"))
        self.assertEqual([task.id for task in reconcile({"a": A_CHECK}, snapshot, identify, {}).cancel], ["5_0"])

    def test_of_two_tasks_doing_one_work_the_claim_holder_is_kept(self):
        snapshot, identify = tasks(("5_0", A, "RUNNING"), ("9_0", A, "RUNNING"))
        decided = reconcile({"a": A}, snapshot, identify, {"a": "9_0"})
        self.assertEqual(decided.waits["a"].id, "9_0")
        self.assertEqual([task.id for task in decided.cancel], ["5_0"])

    def test_without_a_claim_the_first_slurm_took_is_kept(self):
        snapshot, identify = tasks(("9_0", A, "PENDING"), ("5_3", A, "PENDING"))
        decided = reconcile({"a": A}, snapshot, identify, {})
        self.assertEqual(decided.waits["a"].id, "5_3")
        self.assertEqual([task.id for task in decided.cancel], ["9_0"])

    def test_a_task_without_an_identified_intent_is_left_alone(self):
        snapshot, identify = tasks(("5_0", None, "PENDING"))
        self.assertEqual(reconcile({}, snapshot, identify, {}).cancel, ())

    def test_ended_tasks_are_neither_waited_on_nor_cancelled(self):
        snapshot, identify = tasks(("5_0", A, "COMPLETED"), ("6_0", B, "TIMEOUT"))
        decided = reconcile({"a": A}, snapshot, identify, {})
        self.assertEqual((decided.waits, decided.cancel, decided.free), ({}, (), frozenset({"a"})))

    def test_the_same_inputs_give_the_same_decision_in_any_order(self):
        rows = [("9_0", A, "PENDING"), ("5_1", A, "RUNNING"), ("6_0", B, "RUNNING"), ("7_2", A2, "PENDING")]
        first, identify = tasks(*rows)
        second, _ = tasks(*reversed(rows))
        wanted = {"a": A, "b": B}
        self.assertEqual(reconcile(wanted, first, identify, {}), reconcile(wanted, second, identify, {}))

    def test_a_claim_requires_proof_of_terminal_state_for_the_same_intent(self):
        snapshot, _ = tasks(("5_0", A, "RUNNING"), ("6_0", B, "COMPLETED"))
        self.assertEqual(releasable({"i-0": "6_0", "other-0": "6_0", "missing-0": "8_0"}, snapshot),
                         frozenset({"i-0"}))

    def test_the_newest_task_by_slurms_order(self):
        self.assertEqual(newest(["9_2", "10_0", "9_10"]), "10_0")
        self.assertEqual(newest(["9_2", "9_10"]), "9_10")


if __name__ == "__main__":
    unittest.main()
