# One image for every node of the test cluster; `entrypoint.sh` starts what a node's role needs.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq \
    && apt-get install -y -qq --no-install-recommends \
        slurmctld slurmd slurm-client slurmdbd munge mariadb-server \
        openssh-server openssh-client sshfs fuse3 \
        python3 python3-pip python3-venv libfaketime procps iproute2 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# The same user, with the same uid, on every node, as on a cluster: jobs run as it, and it owns the shared files.
RUN useradd --create-home --uid 2000 --shell /bin/bash hpc \
    && echo user_allow_other >> /etc/fuse.conf \
    && mkdir -p /run/sshd /var/spool/slurmctld /var/spool/slurmd /var/log/slurm /run/munge /export /shared \
    && chown slurm:slurm /var/spool/slurmctld /var/log/slurm \
    && chown hpc:hpc /export

# One munge key and one SSH key, built into the image, so every node trusts the others.
RUN ( [ -f /etc/munge/munge.key ] || mungekey --create ) \
    && chown munge:munge /etc/munge/munge.key && chmod 400 /etc/munge/munge.key \
    && su hpc -c 'mkdir -p ~/.ssh && ssh-keygen -q -t ed25519 -N "" -f ~/.ssh/id_ed25519 \
        && cp ~/.ssh/id_ed25519.pub ~/.ssh/authorized_keys \
        && printf "Host *\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n" > ~/.ssh/config' \
    && mkdir -p /root/.ssh && cp /home/hpc/.ssh/id_ed25519 /home/hpc/.ssh/config /root/.ssh/

COPY slurm.conf cgroup.conf slurmdbd.conf /etc/slurm/
RUN chown slurm:slurm /etc/slurm/slurmdbd.conf && chmod 600 /etc/slurm/slurmdbd.conf
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
