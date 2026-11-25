from __future__ import annotations

import os
import subprocess
from pathlib import Path

PROC = Path("/proc")


def group_memory(group: int) -> int:
    if PROC.is_dir():
        return _from_proc(group)
    return _from_ps(group)


def _from_proc(group: int) -> int:
    page = os.sysconf("SC_PAGE_SIZE")
    total = 0
    for entry in os.scandir(PROC):
        if not entry.name.isdigit():
            continue
        try:
            stat = Path(entry.path, "stat").read_text()
        except OSError:
            continue
        fields = stat.rsplit(")", 1)[1].split()
        if int(fields[2]) == group:
            total += int(fields[21]) * page
    return total


def _from_ps(group: int) -> int:
    listing = subprocess.run(["ps", "-A", "-o", "pgid=,rss="], capture_output=True, text=True, check=False).stdout
    total = 0
    for line in listing.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == str(group):
            total += int(parts[1]) * 1024
    return total
