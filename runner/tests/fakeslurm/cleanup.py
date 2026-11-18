"""Keep a fake scheduler's files until its detached workers finish their last write."""

import json
import time
from pathlib import Path


def shutdown(folder: Path, timeout: float = 10.0) -> None:
    """Stop all fake jobs and await every launched array's final filesystem operation.

    A held sbatch array has no worker. Other arrays publish `finished` only
    after joining their task threads and reaping their job processes.
    """
    if not folder.is_dir():
        return
    (folder / "stop").touch()
    pending = {path.stem for path in folder.glob("*.env")
               if not json.loads(path.read_text()).get("held", False)}
    deadline = time.monotonic() + timeout
    while pending:
        pending = {array for array in pending if not (folder / f"{array}.finished").exists()}
        if pending and time.monotonic() >= deadline:
            raise RuntimeError(f"fake Slurm workers did not finish in {folder}: {', '.join(sorted(pending))}")
        if pending:
            time.sleep(0.02)
