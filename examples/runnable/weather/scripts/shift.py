"""Print a mean moved by a scenario's shift: shift.py SHIFT.json MEAN."""

import json
import sys

shift = json.load(open(sys.argv[1]))["shift"]
mean = float(open(sys.argv[2]).read())
print(f"{mean + shift:.2f}")
