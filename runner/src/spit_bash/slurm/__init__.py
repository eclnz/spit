"""Run a DAG's jobs through Slurm: each job is a task of a Slurm array that runs `spit-bash job` on a compute node,
and the run's controller reads the result file each task writes."""
