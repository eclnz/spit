"""A fake Slurm for the runner's tests: sbatch, squeue, sacct, scancel and scontrol, with failures to order.

Its state is a folder, `$FAKE_SLURM_DIR`, holding one JSON file per task. sbatch starts a background process per
array that runs its tasks, as many at once as `%N` and `$FAKE_SLURM_SLOTS` allow. Environment variables, read when
the array is submitted, make tasks fail as a real cluster's can:

- `FAKE_SLURM_VANISH=1`: every task dies before its job starts and leaves no result, ending NODE_FAIL.
- `FAKE_SLURM_OOM_UNDER=MB`: a task with less memory than this dies the same way, ending OUT_OF_MEMORY.
- `FAKE_SLURM_TIMEOUT_UNDER=MINUTES`: a task with less time than this reaches its limit: SIGTERM, SIGKILL five
  seconds later if it still runs, and TIMEOUT.
- `FAKE_SLURM_SCANCEL_FAIL=1`: scancel fails, as with slurmctld unreachable.
- `FAKE_SLURM_SACCT_FAIL=1`: sacct fails, as with accounting off.
- `FAKE_SLURM_HIDE_RUNNING=1`: squeue lists no running task, as if each left the queue before its result shows.
- `FAKE_SLURM_HOLD=1`: tasks stay pending until the file `release` appears in the state folder.
- `FAKE_SLURM_SCANCEL_NOOP=1`: scancel succeeds and cancels nothing.
- `FAKE_SLURM_NO_TERM=1`: a cancelled task is killed with SIGKILL, never SIGTERM.
- `FAKE_SLURM_BLANK_COMMAND=1`: squeue shows `(null)` for a task's command.
- `FAKE_SLURM_SCONTROL_FAIL=1`: scontrol fails.
- `FAKE_SLURM_KILL_WAIT=SECONDS`: the `KillWait` scontrol reports.
- `sbatch --hold` queues tasks that never run.
"""

from __future__ import annotations

import fcntl
import json
import os
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

STATE = Path(os.environ.get("FAKE_SLURM_DIR", "/tmp/fake-slurm"))
ENDED = {"COMPLETED", "FAILED", "CANCELLED", "TIMEOUT", "OUT_OF_MEMORY", "NODE_FAIL"}


def tasks_folder() -> Path:
    folder = STATE / "tasks"
    folder.mkdir(parents=True, exist_ok=True)
    return folder


def load(task: str) -> dict:
    return json.loads((tasks_folder() / f"{task}.json").read_text())


def save(item: dict) -> None:
    path = tasks_folder() / f"{item['task']}.json"
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}")
    temporary.write_text(json.dumps(item))
    os.replace(temporary, path)


def update(task: str, **changes) -> dict:
    with open(STATE / "lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        item = load(task)
        item.update(changes)
        if changes.get("state") in ENDED:
            item["ended"] = time.time()
        save(item)
        if "state" in changes:
            event(task, changes["state"])
        return item


def event(task: str, state: str) -> None:
    """Log a task's change of state with its time, for tests that measure how many tasks were queued at once."""
    with (STATE / "events.log").open("a") as log:
        log.write(json.dumps({"at": time.time(), "task": task, "state": state}) + "\n")


def every() -> list[dict]:
    found = []
    for path in sorted(tasks_folder().glob("*.json")):
        try:
            found.append(json.loads(path.read_text()))
        except (OSError, json.JSONDecodeError):
            pass
    return found


def next_id() -> int:
    STATE.mkdir(parents=True, exist_ok=True)
    with open(STATE / "lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        counter = STATE / "counter"
        number = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(number))
        return number


def option(args: list[str], name: str) -> str | None:
    for arg in args:
        if arg.startswith(f"--{name}="):
            return arg.split("=", 1)[1]
    return None


def sbatch(args: list[str]) -> int:
    if os.environ.get("FAKE_SLURM_SBATCH_FAIL"):
        print("sbatch: error: Batch job submission failed: Invalid account", file=sys.stderr)
        return 1
    if "--test-only" in args:
        if os.environ.get("FAKE_SLURM_TEST_ONLY_FAIL"):
            print("sbatch: error: Batch job submission failed: Requested node configuration is not available",
                  file=sys.stderr)
            return 1
        print("sbatch: Job 1 to start at 2030-01-01T00:00:00 using 1 processors on nodes n1 in partition p",
              file=sys.stderr)
        return 0
    script = args[-1]
    array = option(args, "array") or "0-0"
    span, _, limit = array.partition("%")
    first, _, last = span.partition("-")
    number = next_id()
    name = option(args, "job-name") or "sbatch"
    memory = int((option(args, "mem") or "1024M").rstrip("M"))
    minutes = int(option(args, "time") or "60")
    with (STATE / "submissions.log").open("a") as log:
        log.write(json.dumps({"id": number, "args": args, "at": time.time(), "script": Path(script).read_text()}) + "\n")
    for index in range(int(first), int(last or first) + 1):
        event(f"{number}_{index}", "PENDING")
        save({"task": f"{number}_{index}", "array": number, "index": index, "name": name, "state": "PENDING",
              "reason": "None", "comment": option(args, "comment") or "", "script": script,
              "output": option(args, "output") or f"slurm-{number}_{index}.out", "memory": memory,
              "command": str(Path(script).resolve()),
              "minutes": minutes, "pid": None})
    environment = {key: value for key, value in os.environ.items() if key.startswith("FAKE_SLURM")}
    (STATE / f"{number}.env").write_text(json.dumps(environment))
    if "--hold" in args:
        print(number)
        return 0
    subprocess.Popen([sys.executable, __file__, "_run", str(number), limit or "0"], start_new_session=True,
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print(number)
    return 0


def run_array(number: str, limit: str) -> int:
    """Run one array's tasks, as Slurm's nodes would."""
    environment = json.loads((STATE / f"{number}.env").read_text())
    tasks = sorted((item for item in every() if str(item["array"]) == number), key=lambda item: item["index"])
    slots = int(environment.get("FAKE_SLURM_SLOTS", "4"))
    if int(limit):
        slots = min(slots, int(limit))
    gate = threading.Semaphore(slots)
    threads = [threading.Thread(target=run_task, args=(item, environment, gate)) for item in tasks]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return 0


def run_task(item: dict, environment: dict, gate: threading.Semaphore) -> None:
    task = item["task"]
    while environment.get("FAKE_SLURM_HOLD") and not (STATE / "release").exists():
        if load(task)["state"] == "CANCELLED":
            return
        time.sleep(0.1)
    with gate:
        if load(task)["state"] == "CANCELLED":
            return
        output = item["output"].replace("%A", str(item["array"])).replace("%a", str(item["index"]))
        Path(output).parent.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ, SLURM_ARRAY_TASK_ID=str(item["index"]), SLURM_ARRAY_JOB_ID=str(item["array"]),
                   SLURM_JOB_ID=str(item["array"] * 1000 + item["index"]))
        # A task that dies without a result never gets far enough to write one, however fast its job is.
        dies = ("NODE_FAIL" if environment.get("FAKE_SLURM_VANISH")
                else "OUT_OF_MEMORY" if item["memory"] < int(environment.get("FAKE_SLURM_OOM_UNDER", "0")) else None)
        if dies is not None:
            update(task, state="RUNNING")
            time.sleep(0.2)
            update(task, state=dies)
            return
        with open(output, "w") as stream:
            process = subprocess.Popen(["bash", item["script"]], env=env, stdout=stream, stderr=stream,
                                       start_new_session=True)
        update(task, state="RUNNING", pid=process.pid)
        ending = None
        if item["minutes"] < int(environment.get("FAKE_SLURM_TIMEOUT_UNDER", "0")):
            # As Slurm does at the time limit: SIGTERM, SIGKILL after KillWait, and TIMEOUT whatever the task exits
            # with.
            time.sleep(0.5)
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
            ending = "TIMEOUT"
        while process.poll() is None:
            if load(task)["state"] == "CANCELLED":
                try:
                    os.killpg(process.pid, signal.SIGKILL if environment.get("FAKE_SLURM_NO_TERM") else signal.SIGTERM)
                except ProcessLookupError:
                    pass
                process.wait()
                return
            time.sleep(0.2)
        if ending is None:
            ending = "COMPLETED" if process.returncode == 0 else "FAILED"
        update(task, state=ending)


def squeue(args: list[str]) -> int:
    """As squeue: live tasks, or with `-t all` ended ones too until MinJobAge, filtered by `--name` and `-j`."""
    form = args[args.index("-o") + 1] if "-o" in args else "%i"
    every_state = "-t" in args and args[args.index("-t") + 1] == "all"
    age = float(os.environ.get("FAKE_SLURM_MIN_JOB_AGE", "30"))
    hide = os.environ.get("FAKE_SLURM_HIDE_RUNNING")
    named = next((arg.split("=", 1)[1].split(",") for arg in args if arg.startswith("--name=")), None)
    jobs = args[args.index("-j") + 1].split(",") if "-j" in args else None
    with (STATE / "squeue.log").open("a") as log:
        log.write(json.dumps(args) + "\n")
    for item in every():
        ended = item["state"] in ENDED
        if ended and not (every_state and time.time() - item.get("ended", 0) < age):
            continue
        if hide and item["state"] == "RUNNING":
            continue
        if named is not None and item["name"] not in named:
            continue
        if jobs is not None and item["task"] not in jobs:
            continue
        line = form
        for code, value in (("%i", item["task"]), ("%j", item["name"]), ("%T", item["state"]),
                            ("%r", item["reason"]), ("%k", item["comment"]), ("%K", item["index"]),
                            ("%o", "(null)" if os.environ.get("FAKE_SLURM_BLANK_COMMAND") else item.get("command", item["script"]))):
            line = line.replace(code, str(value))
        print(line)
    return 0


def sacct(args: list[str]) -> int:
    if os.environ.get("FAKE_SLURM_SACCT_FAIL"):
        print("sacct: error: Slurm accounting storage is disabled", file=sys.stderr)
        return 1
    wanted = set(args[args.index("-j") + 1].split(",")) if "-j" in args else None
    with (STATE / "sacct.log").open("a") as log:
        log.write(json.dumps(sorted(wanted or [])) + "\n")
    for item in every():
        if wanted is None or item["task"] in wanted or str(item["array"]) in wanted:
            print(f"{item['task']}|{item['state']}")
    return 0


def scancel(args: list[str]) -> int:
    with (STATE / "scancel.log").open("a") as log:
        log.write(json.dumps(args) + "\n")
    if os.environ.get("FAKE_SLURM_SCANCEL_FAIL"):
        print("scancel: error: Unable to contact slurm controller", file=sys.stderr)
        return 1
    if os.environ.get("FAKE_SLURM_SCANCEL_NOOP"):
        return 0
    named = next((arg.split("=", 1)[1] for arg in args if arg.startswith("--name=")), None)
    for item in every():
        if item["state"] in ENDED:
            continue
        if (named is not None and item["name"] == named) or item["task"] in args or str(item["array"]) in args:
            update(item["task"], state="CANCELLED")
    return 0


def scontrol(args: list[str]) -> int:
    if os.environ.get("FAKE_SLURM_SCONTROL_FAIL"):
        print("scontrol: error: Unable to contact slurm controller", file=sys.stderr)
        return 1
    print(f"KillWait                = {os.environ.get('FAKE_SLURM_KILL_WAIT', '30')} sec")
    print(f"MaxArraySize            = {os.environ.get('FAKE_SLURM_MAX_ARRAY', '1001')}")
    print(f"MinJobAge               = {os.environ.get('FAKE_SLURM_MIN_JOB_AGE', '30')} sec")
    return 0


def main() -> int:
    command, args = sys.argv[1], sys.argv[2:]
    with (STATE / "calls.log").open("a") if STATE.is_dir() else open(os.devnull, "w") as log:
        log.write(command + "\n")
    if command == "_run":
        return run_array(args[0], args[1])
    return {"sbatch": sbatch, "squeue": squeue, "sacct": sacct, "scancel": scancel, "scontrol": scontrol}[command](args)


if __name__ == "__main__":
    raise SystemExit(main())
