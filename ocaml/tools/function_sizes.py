import argparse
import csv
import json
import re
import subprocess
import tempfile
from pathlib import Path

LOCATION = re.compile(r"\[(\d+),(\d+)\+(\d+)\]\.\.\[(\d+),(\d+)\+(\d+)\]")
VARIABLE = re.compile(r'Ppat_var ("(?:[^"\\]|\\.)*")')


def location(text):
    found = LOCATION.search(text)
    if not found:
        raise ValueError(f"Missing compiler source location: {text}")
    first, first_bol, first_col, last, last_bol, last_col = map(int, found.groups())
    return first, last - (last_col == 0 and last > first), first_bol + first_col, last_bol + last_col


def inventory(path, root):
    result = subprocess.run(
        ["ocamlc", "-stop-after", "parsing", "-dparsetree", str(path)],
        capture_output=True,
        text=True,
        check=True,
    )
    dump = [line.strip() for line in result.stderr.splitlines()]
    source = path.read_bytes()
    lines = source.splitlines()
    definitions, modules, functions, bindings = {}, [], [], []
    for i, text in enumerate(dump):
        if text == "Pstr_module" and dump[i + 2].startswith("module_expr ("):
            modules.append((json.loads(dump[i + 1].split('" ', 1)[0] + '"'), location(dump[i + 2])))
        if text != "<def>" or not dump[i + 1].startswith("pattern ("):
            continue
        name = VARIABLE.match(dump[i + 2])
        if name:
            expression = i + 3
            if dump[expression].startswith("expression ("):
                definitions[location(dump[expression])] = (json.loads(name[1]), location(dump[i + 1]))
        elif dump[i + 2].startswith('Ppat_construct "()"') and dump[i + 4].startswith("expression ("):
            definitions[location(dump[i + 4])] = ("<initializer>", location(dump[i + 1]))
    for i, text in enumerate(dump):
        if not text.startswith("expression ("):
            continue
        position = location(text)
        first, last, start, finish = position
        descriptor = dump[i + 1]
        function = descriptor in {"Pexp_function", "Pexp_fun"}
        definition = definitions.get(position)
        if not function and not definition:
            continue
        parts = [path.stem.capitalize()] + [
            name
            for name, span in sorted(modules, key=lambda item: item[1][2])
            if span[2] <= start and finish <= span[3]
        ]
        module = ".".join(parts)
        if definition:
            name, declared = definition
            first, _, start, _ = declared
        else:
            name = f"<lambda@{first}:{start - source.rfind(bytes([10]), 0, start)}>"
        if not function and name != "<initializer>":
            bindings.append(
                {
                    "file": str(path.relative_to(root)),
                    "binding": module + "." + name,
                    "start": first,
                    "end": last,
                    "expression": descriptor,
                }
            )
            continue
        functions.append(
            {
                "file": str(path.relative_to(root)),
                "function": name,
                "kind": "function" if function and definition else "lambda" if function else "initializer",
                "start": first,
                "end": last,
                "lines": last - first + 1,
                "nonblank": sum(bool(line.strip()) for line in lines[first - 1 : last]),
                "max_width": max((len(line.decode().expandtabs(4)) for line in lines[first - 1 : last]), default=0),
                "nested": 0,
                "tests": "tests" in path.relative_to(root).parts,
                "start_byte": start,
                "end_byte": finish,
                "module": module,
            }
        )
    functions.sort(key=lambda row: (row["start_byte"], -row["end_byte"]))
    pending = []
    for index, row in enumerate(functions):
        while pending and row["end_byte"] > functions[pending[-1]]["end_byte"]:
            pending.pop()
        parent = functions[pending[-1]] if pending else None
        row["parent_index"] = pending[-1] if pending else None
        row["parent_function"] = parent["function"] if parent else ""
        row["function"] = (
            (parent["function"] if parent and parent["module"] == row["module"] else row["module"])
            + "."
            + row["function"]
        )
        if parent:
            parent["nested"] += 1
        pending.append(index)
    for index, row in enumerate(functions):
        covered = {
            line
            for child in functions
            if child["parent_index"] == index
            for line in range(child["start"], child["end"] + 1)
        }
        row["lines_outside_nested_ranges"] = sum(
            bool(lines[line - 1].strip()) for line in range(row["start"], row["end"] + 1) if line not in covered
        )
    for row in functions:
        for key in ["parent_index", "start_byte", "end_byte", "module"]:
            row.pop(key)
    expected = sum(text in {"Pexp_function", "Pexp_fun"} for text in dump)
    if sum(row["kind"] != "initializer" for row in functions) != expected:
        raise ValueError(f"Unaccounted function nodes in {path}")
    return functions, bindings


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    output = args.output or Path(tempfile.mkdtemp(prefix="spit-function-audit-"))
    output.mkdir(parents=True, exist_ok=True)
    functions, bindings = [], []
    paths = sorted(root.rglob("*.ml"))
    for path in paths:
        rows, values = inventory(path, root)
        functions.extend(rows)
        bindings.extend(values)
    functions.sort(key=lambda row: (-row["lines"], row["file"], row["start"]))
    for name, rows in [("functions", functions), ("bindings", bindings)]:
        (output / f"{name}.json").write_text(json.dumps(rows, indent=2) + "\n")
        if rows:
            with (output / f"{name}.csv").open("w", newline="") as stream:
                writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
                writer.writeheader()
                writer.writerows(rows)
    production = [row for row in functions if not row["tests"] and row["kind"] != "initializer"]
    summary = {
        "files": len(paths),
        "production_functions": len(production),
        "named_production_functions": sum(row["kind"] == "function" for row in production),
        "production_functions_over_40_lines": sum(row["lines"] > 40 for row in production),
        "test_functions": sum(row["tests"] and row["kind"] != "initializer" for row in functions),
        "initializers": sum(row["kind"] == "initializer" for row in functions),
        "output": str(output),
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    for row in production:
        if row["lines"] > 40:
            print(f"{row['lines']:4} lines {row['nested']:2} nested  {row['function']}  {row['file']}:{row['start']}")


if __name__ == "__main__":
    main()
