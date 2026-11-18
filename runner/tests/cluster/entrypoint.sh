#!/bin/bash
# Start one node of the test cluster. The first argument is the node's role:
#   storage   the file server: exports /export over SSH
#   db        MariaDB and slurmdbd, so that sacct has records
#   login     slurmctld, and where a person or a test runs sbatch and spit-bash
#   compute   slurmd
# login and compute nodes mount storage:/export at /shared over sshfs. Each node caches what it
# has seen for CACHE_SECONDS, as an NFS client does with actimeo, so a file one node writes takes
# that long to appear, change or vanish on another.
set -euo pipefail
role=${1:?usage: entrypoint.sh storage|db|login|compute}
cache=${CACHE_SECONDS:-30}

start_munge() {
    install -d -o munge -g munge /run/munge
    runuser -u munge -- munged --force
}

wait_for() {  # host port
    for _ in $(seq 1 120); do
        (exec 3<>"/dev/tcp/$1/$2") 2>/dev/null && return 0
        sleep 1
    done
    echo "entrypoint: $1:$2 did not answer" >&2
    exit 1
}

delegate_cgroups() {
    # Under cgroup v2 a cgroup with processes of its own cannot hand controllers to cgroups below it, so the
    # container's processes move to a cgroup of their own, and slurmd gets the memory and CPU controllers to give
    # each job. The container's cgroup is the root of a private namespace, or a folder of the host's tree.
    # Under v1 there is nothing to do.
    [ -f /sys/fs/cgroup/cgroup.controllers ] || return 0
    local own=/sys/fs/cgroup$(sed -n 's/^0:://p' /proc/self/cgroup)
    mkdir -p "$own/init"
    while read -r process; do
        echo "$process" > "$own/init/cgroup.procs" 2>/dev/null || true
    done < "$own/cgroup.procs"
    for controller in cpu cpuset memory pids; do
        echo "+$controller" > "$own/cgroup.subtree_control" 2>/dev/null || true
    done
    # slurmd makes its stepd scope under system.slice, which only systemd creates. Each slurmstepd then looks for
    # the same scope below the one slurmd moved it into, so both levels are made, with the controllers handed down.
    local scope level path=$own
    scope=$(hostname)_slurmstepd.scope
    for level in system.slice "$scope" system.slice "$scope"; do
        path=$path/$level
        mkdir -p "$path"
        for controller in cpu cpuset memory pids; do
            echo "+$controller" > "$path/cgroup.subtree_control" 2>/dev/null || true
        done
    done
}

mount_shared() {
    wait_for storage 22
    sshfs hpc@storage:/export /shared -o "allow_other,reconnect,ServerAliveInterval=15,\
attr_timeout=$cache,entry_timeout=$cache,negative_timeout=$cache,dir_cache=yes,dcache_timeout=$cache"
}

case "$role" in
storage)
    exec /usr/sbin/sshd -D -e
    ;;
db)
    start_munge
    mkdir -p /run/mysqld && chown mysql:mysql /run/mysqld
    mysqld_safe --user=mysql &
    for _ in $(seq 1 60); do mysqladmin ping --silent 2>/dev/null && break; sleep 1; done
    mysql -e "CREATE DATABASE IF NOT EXISTS slurm_acct_db;
              CREATE USER IF NOT EXISTS 'slurm'@'localhost' IDENTIFIED BY 'slurm';
              GRANT ALL ON slurm_acct_db.* TO 'slurm'@'localhost';"
    exec runuser -u slurm -- slurmdbd -D
    ;;
login)
    start_munge
    mount_shared
    wait_for db 6819
    sacctmgr -i add cluster spit-test >/dev/null || true
    sacctmgr -i add account spit Description=tests Organization=spit >/dev/null || true
    sacctmgr -i add user hpc Account=spit >/dev/null || true
    exec runuser -u slurm -- slurmctld -D -i
    ;;
compute)
    delegate_cgroups
    start_munge
    mount_shared
    wait_for login 6817
    exec slurmd -D
    ;;
*)
    echo "entrypoint: unknown role $role" >&2
    exit 2
    ;;
esac
