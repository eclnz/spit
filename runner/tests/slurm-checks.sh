#!/bin/bash
# The checks of spit-bash's Slurm executor that take too long for GitHub, to run on a developer's machine before a
# change under runner/src/spit_bash/slurm/ merges. GitHub runs only the fast fake-Slurm tests, test_slurm.py and
# test_slurm_job.py, with the rest of the runner's tests.
#
#   runner/tests/slurm-checks.sh            # fake Slurm, scale, then the Docker cluster if Docker runs
#   runner/tests/slurm-checks.sh --no-cluster
#
# The fake-Slurm tests take about a minute, the scale test (2,001 jobs) about ten, and the Docker cluster about
# twenty-five. Prints a line for each check, and exits non-zero if any failed.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cluster=yes
[ "${1:-}" = "--no-cluster" ] && cluster=no
results=()
failed=0

step() {  # name command...
    local name=$1
    shift
    echo "== $name"
    if "$@"; then
        results+=("ok    $name")
    else
        results+=("FAIL  $name")
        failed=1
    fi
}

step "fake Slurm tests" python3 -m unittest discover -s "$here" -p 'test_slurm*.py'
step "Slurm submission state model" python3 "$here/slurm_state_model.py"
step "Slurm guarantees under scheduler failures" python3 "$here/slurm_guarantees.py"
step "scale: 2,001 jobs, and 100 stopped at their limit" \
    env SPIT_BASH_SCALE=1 python3 -m unittest discover -s "$here" -p test_slurm_scale.py -v

if [ "$cluster" = no ]; then
    results+=("skip  Docker cluster: --no-cluster")
elif ! docker info >/dev/null 2>&1; then
    results+=("skip  Docker cluster: no Docker daemon; start one, as runner/tests/cluster/README.md says")
else
    up() {
        docker compose -f "$here/cluster/compose.yaml" up -d --build || return 1
        for _ in $(seq 1 90); do
            docker compose -f "$here/cluster/compose.yaml" exec -T login sinfo -h -o %t 2>/dev/null | grep -q idle \
                && return 0
            sleep 2
        done
        return 1
    }
    step "Docker cluster starts" up
    step "SPIT and spit-bash install on it" "$here/cluster/install.sh"
    step "the cluster works" "$here/cluster/smoke.sh"
    step "spit-bash runs on it" python3 "$here/cluster/check.py"
fi

echo
printf '%s\n' "${results[@]}"
exit $failed
