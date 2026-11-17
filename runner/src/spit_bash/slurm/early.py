"""A SIGTERM that reaches `spit-bash job` before it is ready for one, recorded rather than fatal.

Slurm can send a task's time-limit warning while Python is still starting, which on a busy shared filesystem can
take seconds. Python's default would end the task at once, writing no result, and the controller would wait
`result_wait` for one. The batch script ignores SIGTERM until Python runs; `__main__` installs `record` before
importing anything else; and the task, once ready, puts back nothing it has not yet touched and says it was
terminated.
"""

import signal

received = False


def record(number: int, frame: object) -> None:
    global received
    received = True


def install() -> None:
    signal.signal(signal.SIGTERM, record)
