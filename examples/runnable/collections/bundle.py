"""Bundle the named files and their contents, retaining the paths used."""
import json
from pathlib import Path
import sys

mode, argument, output = sys.argv[1:]
paths = argument.split(',') if mode == 'joined' else Path(argument).read_text().splitlines()
Path(output).write_text(json.dumps([{'path': p, 'text': Path(p).read_text()} for p in paths]) + '\n')
