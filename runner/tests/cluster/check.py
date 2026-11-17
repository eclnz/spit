#!/usr/bin/env python3
"""Run spit-bash with `--executor slurm` on the test cluster, and check what only a real cluster shows: jobs on
other nodes behind a lagging shared folder, cgroup memory kills, accounting down, and a controller killed mid-run.

Run install.sh first. Each check prints `ok` or stops with what went wrong. Name checks to run only those, as
`check.py a_new_controller_takes_over`. Runs from anywhere.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
COMPOSE = ["docker", "compose", "-f", str(HERE / "compose.yaml")]
ENV = "export PATH=/shared/spit/bin:/shared/spit/venv/bin:$PATH SPIT=/shared/spit/bin/spit; "
PIPELINES = "/shared/spit/runner/tests/cluster/pipelines"
# The nodes cache the shared folder for 30 seconds, so a result can take that long to appear on the login node.
PROFILE = """\
status_interval = "5s"
result_interval = "2s"
result_wait = "90s"
gather = "2s"
partition = "compute"
memory = "200M"
time = "10m"
retries = 1
"""


def on(node: str, script: str, check: bool = False, timeout: float = 600) -> subprocess.CompletedProcess[str]:
    done = subprocess.run([*COMPOSE, "exec", "-T", "-u", "hpc", "-w", "/shared", node, "bash", "-c", ENV + script],
                          capture_output=True, text=True, timeout=timeout)
    if check and done.returncode != 0:
        fail(f"`{script}` on {node} exited {done.returncode}:\n{done.stdout}\n{done.stderr}")
    return done


def fail(words: str) -> None:
    print(f"FAIL {words}", file=sys.stderr)
    raise SystemExit(1)


def ok(words: str) -> None:
    print(f"ok   {words}", flush=True)


def expect(words: str, wanted: str, got: str) -> None:
    if wanted not in got:
        fail(f"{words}: expected {wanted!r} in:\n{got}")
    ok(words)


def fresh(name: str, pipeline: str | None = None, notes: tuple[str, ...] = ("a",)) -> str:
    """A new folder under /shared/check for one check, with a pipeline and notes; its path.

    Each is new, never a folder deleted and made again: the nodes' caches would list what the old one held.
    """
    folder = f"/shared/check/{name}-{time.strftime('%H%M%S')}"
    lines = [f"mkdir -p {folder}/data", f"printf '%s' '{PROFILE}' > {folder}/cluster.toml"]
    if pipeline is not None:
        lines.append(f"cp {PIPELINES}/{pipeline} {folder}/")
    lines += [f"echo {note} > {folder}/data/{note}.txt" for note in notes]
    on("login", " && ".join(lines), check=True)
    return folder


def run(folder: str, target: str, *more: str, timeout: float = 600) -> subprocess.CompletedProcess[str]:
    return on("login", f"cd {folder} && spit-bash run {target} --executor slurm --profile cluster.toml {' '.join(more)}",
              timeout=timeout)


def examples_run_on_other_nodes() -> None:
    folder = fresh("lines", notes=())
    on("login", f"cp -r /shared/spit/examples/lines/. {folder}/ && rm -rf {folder}/output {folder}/.spit-bash", check=True)
    done = run(folder, "lines.spitin")
    expect("examples/runnable/lines runs through Slurm", "5 done, 0 checked, 0 failed", done.stderr)
    expect("its jobs run on the compute nodes", "Slurm task", done.stdout)
    # The login node looked the outputs up before the jobs made them, and caches that they were missing.
    time.sleep(35)
    expect("the login node judges their outputs current", "all 5 jobs are current",
           on("login", f"cd {folder} && spit-bash plan lines.spitin").stdout)
    time.sleep(35)
    expect("so does another node, by content", "all 5 jobs are current",
           on("c2", f"cd {folder} && spit-bash plan lines.spitin").stdout)
    on("login", f"echo zebra > {folder}/input/beta/01.txt", check=True)
    expect("a local run after a Slurm run reruns only what changed", "2 done",
           on("login", f"cd {folder} && spit-bash run lines.spitin").stderr)


def weather_runs_as_it_does_locally() -> None:
    """Every version of the weather family, run locally and then again through Slurm, makes the same files."""
    folder = fresh("weather", notes=())
    on("login", f"cp -r /shared/spit/examples/weather/. {folder}/ && rm -rf {folder}/output", check=True)
    versions = ("v1_means", "v2_checked", "v3_scenarios", "v4_survey")
    for version in versions:
        done = on("login", f"cd {folder} && spit-bash run {version}/{version}.spitin -j 2")
        if done.returncode != 0:
            fail(f"weather {version} failed locally:\n{done.stderr}")
    on("login", f"cd {folder} && cp -r output output-local", check=True)
    for version in versions:
        done = run(folder, f"{version}/{version}.spitin", "--force", "--yes")
        if done.returncode != 0:
            fail(f"weather {version} failed through Slurm:\n{done.stdout}\n{done.stderr}")
        expect(f"weather {version} runs through Slurm", "0 failed", done.stderr)
    differ = on("login", f"cd {folder} && diff -r -x .spit-bash output-local output")
    if differ.returncode != 0:
        fail(f"the Slurm run made different files from the local run:\n{differ.stdout}")
    ok("every weather output is the same, byte for byte, as the local run's")


def memory_kills_are_retried() -> None:
    folder = fresh("hog", "hog.spit")
    done = run(folder, "hog.spit")
    expect("a job over its memory is killed by its cgroup and retried with more", "retry", done.stdout)
    expect("the retry succeeds", "1 done", done.stderr)
    accounting = on("login", "sacct -X -n -P --name=$(sacct -X -n -P --format=JobName | grep -- '-hog$' | tail -1) "
                             "--format=State,ReqMem").stdout
    expect("sacct shows the kill and the larger request", "OUT_OF_MEMORY", accounting)


def time_limits_are_retried() -> None:
    """A job Slurm stops before its time limit puts its outputs back, exits, and is retried with twice the time.

    The job handles the SIGTERM that `--signal` sends and exits 0, so sacct calls it COMPLETED, not TIMEOUT: the
    runner must read a terminated task that completed as stopped at its limit.
    """
    folder = fresh("sleepy", "sleepy.spit")
    on("login", f"sed -i 's/^time = .*/time = \"2m\"/' {folder}/cluster.toml", check=True)
    done = run(folder, "sleepy.spit", timeout=900)
    expect("a job stopped before its time limit is retried", "at its time limit", done.stdout)
    expect("with twice the time", "4 minutes", done.stdout)
    expect("and the retry succeeds", "1 done", done.stderr)
    first = on("login", "sacct -X -n -P --format=JobID,State,Timelimit --name=$(sacct -X -n -P --format=JobName "
                        "| grep -- '-sleepy$' | tail -1)").stdout
    expect("sacct calls the stopped attempt COMPLETED, as the runner expects", "COMPLETED|00:02:00", first)


def a_run_goes_on_without_accounting() -> None:
    folder = fresh("noacct", "slow.spit")
    subprocess.run([*COMPOSE, "stop", "db"], capture_output=True, check=True)
    try:
        on("login", f"sed -i 's/sleep 20/sleep 1/' {folder}/slow.spit", check=True)
        done = run(folder, "slow.spit")
        expect("a run goes on with slurmdbd down", "1 done", done.stderr)
    finally:
        subprocess.run([*COMPOSE, "start", "db"], capture_output=True, check=True)


def a_new_controller_takes_over() -> None:
    folder = fresh("restart", "slow.spit", notes=("a", "b"))
    command = f"cd {folder} && exec spit-bash run slow.spit --executor slurm --profile cluster.toml"
    started = subprocess.Popen([*COMPOSE, "exec", "-T", "-u", "hpc", "-w", "/shared", "login", "bash", "-c",
                                ENV + command], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    # The pipeline's root is `data`, which holds the runner's folder.
    ledger = f"{folder}/data/.spit-bash/slurm/ledger.jsonl"
    deadline = time.time() + 60
    while time.time() < deadline and not on("login", f"test -s {ledger}").returncode == 0:
        time.sleep(1)
    # As a login node's reaper or a lost SSH session would: no chance to cancel anything.
    on("login", "pkill -KILL -f 'spit-bash run slow.spit'")
    started.communicate(timeout=60)
    submitted = on("login", f"cat {ledger}", check=True).stdout.splitlines()
    tasks = {json.loads(line)["task"] for line in submitted if "task" in json.loads(line)}
    done = run(folder, "slow.spit")
    expect("the next controller finds the queued tasks", "still queued or running", done.stderr)
    expect("and waits on them, not on new ones", "2 done", done.stderr)
    queued = [line.split(": Slurm task ")[1] for line in done.stdout.splitlines() if ": Slurm task " in line]
    if set(queued) != tasks:
        fail(f"the second controller queued {queued}, where the first submitted {sorted(tasks)}")
    ok("no job was submitted twice")


CHECKS = (examples_run_on_other_nodes, weather_runs_as_it_does_locally, memory_kills_are_retried,
          time_limits_are_retried, a_run_goes_on_without_accounting, a_new_controller_takes_over)


def main() -> int:
    """Run every check, or those named on the command line."""
    chosen = sys.argv[1:]
    for check in CHECKS:
        if not chosen or check.__name__ in chosen:
            check()
    print("spit-bash runs on the cluster")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
