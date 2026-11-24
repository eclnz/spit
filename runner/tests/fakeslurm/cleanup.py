import json
import time
from pathlib import Path


def shutdown(folder: Path, timeout: float = 10.0) -> None:
    if not folder.is_dir():
        return
    (folder / "stop").touch()
    pending = {path.stem for path in folder.glob("*.env") if not json.loads(path.read_text()).get("held", False)}
    deadline = time.monotonic() + timeout
    while pending:
        pending = {array for array in pending if not (folder / f"{array}.finished").exists()}
        if pending and time.monotonic() >= deadline:
            raise RuntimeError(f"fake Slurm workers did not finish in {folder}: {', '.join(sorted(pending))}")
        if pending:
            time.sleep(0.02)
