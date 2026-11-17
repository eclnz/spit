#!/bin/bash
# Check that the running test cluster behaves as a cluster: Slurm runs jobs and arrays on the compute
# nodes, sacct records them, cgroups kill a job over its memory, one node sees another's files only
# after the cache expires, and spit-bash runs a runnable example in a job. Run install.sh first.
# Exits non-zero at the first check that fails.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cache=${CACHE_SECONDS:-30}
on() {  # node command...
    local node=$1; shift
    docker compose -f "$here/compose.yaml" exec -T -u hpc -w /shared "$node" bash -c "$*"
}
check() {  # what expected actual
    if [[ "$3" != *"$2"* ]]; then
        echo "FAIL $1: expected '$2', got '$3'" >&2
        exit 1
    fi
    echo "ok   $1"
}
wait_job() {  # job id
    for _ in $(seq 1 120); do
        [[ -z $(on login "squeue -h -j $1 2>/dev/null") ]] && return 0
        sleep 1
    done
    echo "FAIL job $1 did not leave the queue" >&2
    exit 1
}
state() {  # job id; sacct may lag the job's end by a moment
    for _ in $(seq 1 30); do
        local found
        found=$(on login "sacct -X -n -P -j $1 --format=State" | sort -u | tr '\n' ' ')
        [[ -n $found && $found != *RUNNING* && $found != *PENDING* ]] && { echo "$found"; return; }
        sleep 1
    done
}
run=smoke-$$
on login "mkdir -p $run"

id=$(on login "sbatch --parsable -c 1 --mem=100M -J spit-smoke -o $run/host.out --wrap 'hostname'")
wait_job "$id"
check "a job runs on a compute node" "c" "$(on login "cat $run/host.out")"
check "sacct records it" "COMPLETED" "$(state "$id")"

id=$(on login "sbatch --parsable -c 1 --mem=100M -J spit-smoke -o /dev/null \
    --wrap 'python3 -c \"x = bytearray(400 << 20); import time; time.sleep(2)\"'")
wait_job "$id"
check "a job over its memory is killed" "OUT_OF_MEMORY" "$(state "$id")"

id=$(on login "sbatch --parsable --array=0-3%2 -c 1 --mem=50M -J spit-smoke -o $run/task.%a.out --wrap 'echo \$SLURM_ARRAY_TASK_ID'")
wait_job "$id"
check "an array runs every task" "0 1 2 3" "$(on login "cat $run/task.*.out | sort | tr '\n' ' '")"

on login "test ! -e $run/late.txt"  # login now caches that the file is missing
on c1 "echo late > $run/late.txt"
check "another node does not see a new file at once" "missing" "$(on login "cat $run/late.txt 2>/dev/null || echo missing")"
sleep $((cache + 5))
check "it sees it once the cache expires" "late" "$(on login "cat $run/late.txt")"

id=$(on login "sbatch --parsable -c 2 --mem=500M -J spit-smoke -o $run/lines.out --wrap \
    'export PATH=/shared/spit/bin:/shared/spit/venv/bin:\$PATH SPIT=/shared/spit/bin/spit; \
     spit-bash run /shared/spit/examples/lines/lines.spitin --force -j 2'")
wait_job "$id"
check "spit-bash runs an example in a job" "0 failed" "$(on login "cat $run/lines.out")"

on login "rm -rf $run"
echo "the cluster works"
