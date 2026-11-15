"""Print one row for each result: its place, its scenario and its value.

usage: tabulate.py join FILE,FILE,... --places PLACE... --scenarios SCENARIO...
       tabulate.py list LISTFILE --places PLACE... --scenarios SCENARIO...
"""

import argparse

parser = argparse.ArgumentParser()
parser.add_argument("mode", choices=["join", "list"])
parser.add_argument("files")
parser.add_argument("--places", nargs="+", required=True)
parser.add_argument("--scenarios", nargs="+", required=True)
arguments = parser.parse_args()

paths = arguments.files.split(",") if arguments.mode == "join" else open(arguments.files).read().split()
print("place\tscenario\tmean")
for place, scenario, path in zip(arguments.places, arguments.scenarios, paths):
    print(f"{place}\t{scenario}\t{float(open(path).read()):.1f}")
