"""Generated failure sequences exercise the production Slurm controller and persisted budgets."""

import asyncio
import json
import math
import tempfile
from pathlib import Path
from unittest.mock import patch

from hypothesis import HealthCheck, event, settings, strategies as st
from hypothesis.stateful import RuleBasedStateMachine, invariant, precondition, rule

from spit_bash.execute.executor import Hooks, Task as Work
from spit_bash.execute.options import Options
from spit_bash.model import DagError, Layout
from spit_bash.slurm import commands, result
from spit_bash.slurm.controller import Controller
from spit_bash.slurm.dispatch import Dispatches
from spit_bash.slurm.executor import Slurm, Want
from spit_bash.slurm.profile import Profile
from spit_bash.slurm.reconcile import Identity, Mode
from spit_bash.slurm.throttle import SubmitRate
from spit_bash.state.store import Store
from support import document, job_entry, new_run, read


class Crash(BaseException):
    """The scheduler accepts a call whose controller never records its acknowledgement."""


class SubmissionSequences(RuleBasedStateMachine):
    def __init__(self):
        super().__init__()
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.layout = Layout(self.root / "runner")
        self.folder = self.layout.folder / "slurm"
        self.profile = Profile(jobs=3, submissions=3, per=10.0)
        self.now = 100.0
        self.host = "host-1"
        self.outcome = "ack"
        self.arrays = {}
        self.calls = []
        self.executor = None
        self.loop = asyncio.new_event_loop()
        entries = [dict(job_entry(index, ["source.txt"], f"out-{index}.txt", "pass"), operation="copy")
                   for index in range(1, 4)]
        self.dag = read(document(self.root, entries, ["source.txt"]))
        sleep = asyncio.sleep

        async def advance(seconds):
            self.now = math.nextafter(self.now + seconds, math.inf)
            await sleep(0)

        self.patches = [patch("time.monotonic", side_effect=lambda: self.now),
                        patch("time.time", side_effect=lambda: 1e9 + self.now),
                        patch("socket.gethostname", side_effect=lambda: self.host),
                        patch("asyncio.sleep", side_effect=advance),
                        patch.object(commands, "submit", side_effect=self.scheduler),
                        patch.object(commands, "snapshot", side_effect=lambda *args: self.rows()),
                        patch.object(commands, "accounting", return_value=None),
                        patch.object(commands, "cancel", return_value=[])]
        for patched in self.patches:
            patched.start()
        try:
            self.restart()
        except BaseException:
            self.teardown()
            raise

    def rows(self, hide=0, foreign=False):
        return [commands.Listed(f"{array}_{index}", index, state,
                                str((self.root if foreign else self.folder / "intents") / f"{item['name']}.sh"))
                for array, item in self.arrays.items() for index, state in enumerate(item["states"])
                if not hide & (1 << index)]

    async def scheduler(self, script, arguments):
        name = script.stem
        manifest = json.loads(script.with_suffix(".json").read_text())
        event(f"scheduler outcome={self.outcome}")
        self.calls.append((self.now, name))
        assert f"--array=0-{len(manifest) - 1}" in arguments
        assert "--no-requeue" in arguments
        if self.outcome == "refused":
            raise commands.SlurmError("sbatch refused the submission: Invalid account")
        array = str(len(self.arrays) + 1)
        self.arrays[array] = dict(name=name, keys=[each[0] for each in manifest],
                                  states=["PENDING"] * len(manifest))
        if self.outcome == "crash":
            raise Crash()
        if self.outcome == "lost":
            raise commands.Unsure("accepted, reply lost")
        return array

    @rule()
    def restart(self):
        if self.executor is not None:
            for want in self.executor.wants.values():
                want.future.cancel()
        self.executor = None
        controller = Controller(self.profile, self.layout)
        with patch.object(commands, "config", return_value=commands.Config(max_array=2)):
            try:
                start = controller.start(self.dag, Store(self.layout.folder / "state.jsonl", write=False), write=True)
            except DagError:
                event("restart blocked by unresolved evidence")
                return
        planned = {job.work.key: Identity(job.work.key, job.work.fingerprint, Mode.CHECK) for job in self.dag.jobs}
        options = Options(new_run(self.layout.runs), 1, None)
        self.executor = Slurm(self.profile, self.dag, options, self.folder, {}, {}, {}, planned,
                              start.tasks, start.held, commands.Config(max_array=2))
        for job in self.dag.jobs:
            self.executor.wants[job.work.key] = Want(
                job, planned[job.work.key], Work(job), options.run.folder / f"{job.id}.log", 1024, 60.0,
                self.profile.resources, Hooks(), self.loop.create_future(), float("-inf"))

    @rule(seconds=st.integers(min_value=0, max_value=12))
    def advance_time(self, seconds):
        self.now += seconds

    @rule(other_host=st.booleans())
    def change_host(self, other_host):
        self.host = "host-2" if other_host else "host-1"
        self.restart()

    @precondition(lambda self: self.executor is not None)
    @rule(outcome=st.sampled_from(["ack", "ack", "ack", "refused", "lost", "crash", "before_send"]))
    def submit_ready_work(self, outcome):
        self.outcome = outcome
        charge = SubmitRate.charge

        def interrupted_charge(limiter):
            charge(limiter)
            raise Crash()

        try:
            if outcome == "before_send":
                with patch.object(SubmitRate, "charge", interrupted_charge):
                    self.loop.run_until_complete(self.executor._submit_free())
            else:
                self.loop.run_until_complete(self.executor._submit_free())
        except Crash:
            self.restart()

    @precondition(lambda self: bool(self.arrays))
    @rule(array_index=st.integers(min_value=0, max_value=20), element=st.integers(min_value=0, max_value=1))
    def scheduler_progresses(self, array_index, element):
        item = list(self.arrays.values())[array_index % len(self.arrays)]
        index = element % len(item["states"])
        states = {"PENDING": "RUNNING", "RUNNING": "COMPLETING", "COMPLETING": "CANCELLED",
                  "CANCELLED": "CANCELLED"}
        item["states"][index] = states[item["states"][index]]
        event(f"physical state={item['states'][index]}")

    @precondition(lambda self: self.executor is not None)
    @rule(hide=st.integers(min_value=0, max_value=3), foreign=st.booleans(), unavailable=st.booleans())
    def observe_partial_queue(self, hide, foreign, unavailable):
        with patch.object(commands, "snapshot", return_value=None if unavailable else self.rows(hide, foreign)):
            self.loop.run_until_complete(self.executor._observe())

    @precondition(lambda self: self.executor is not None)
    @rule()
    def worker_receipts_arrive(self):
        for array, item in self.arrays.items():
            for index, key in enumerate(item["keys"]):
                want = self.executor.wants[key]
                receipt = result.Result(f"{array}_{index}", item["name"], index, self.executor.options.run.name,
                                        want.job.id, key, want.identity.fingerprint, result.Outcome.CHECKED, self.host)
                result.write(receipt, result.path(self.folder, receipt.task))
        self.executor._read_results()

    @invariant()
    def physical_allocations_have_reservations_and_calls_stay_bounded(self):
        reservations = Dispatches(self.folder)
        actual_live = 0
        charged = sum(len(each.left) for each in reservations.records.values())
        live_keys = []
        for item in self.arrays.values():
            live = [index for index, state in enumerate(item["states"]) if state != "CANCELLED"]
            actual_live += len(live)
            live_keys.extend(item["keys"][index] for index in live)
            record = reservations.records.get(item["name"])
            assert not live or record is not None and set(live) <= set(record.left)
        assert actual_live <= charged <= self.profile.jobs
        assert len(live_keys) == len(set(live_keys))
        assert len({name for _, name in self.calls}) == len(self.calls)
        times = [at for at, _ in self.calls]
        assert all(later - earlier >= self.profile.per / self.profile.submissions - 1e-9
                   for earlier, later in zip(times, times[1:]))
        assert all(sum(start <= at < start + self.profile.per for at in times) <= self.profile.submissions
                   for start in times)

    def teardown(self):
        for patched in reversed(self.patches):
            patched.stop()
        self.loop.run_until_complete(self.loop.shutdown_default_executor())
        self.loop.close()
        self.temporary.cleanup()


TestSubmissionSequences = SubmissionSequences.TestCase
TestSubmissionSequences.settings = settings(max_examples=100, stateful_step_count=40, deadline=None,
                                            suppress_health_check=[HealthCheck.too_slow])
