"""Write a report and its summary side by side: report.py BOARD FOLDER STEM."""

import sys
from pathlib import Path

board, folder, stem = sys.argv[1:4]
rows = [line.split("\t") for line in Path(board).read_text().splitlines()[1:]]
warmest = max(rows, key=lambda row: float(row[2]))
Path(folder, f"{stem}.txt").write_text("".join("{} {} {}\n".format(*row) for row in rows))
Path(folder, f"{stem}_summary.txt").write_text(f"{len(rows)} results, warmest {warmest[0]} under {warmest[1]}\n")
