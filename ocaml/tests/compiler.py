import json
import os
from pathlib import Path
import random
import shutil
import subprocess
import sys
import tempfile
import time

compiler = Path(sys.argv[1])
repository = Path(sys.argv[2])
runner_name = os.environ.get("SPIT_BASH", str(repository / ".venv/bin/spit-bash"))
runner = Path(shutil.which(runner_name) or runner_name)
checks = 0


def run(arguments, success=True):
    global checks
    result = subprocess.run([str(compiler), *map(str, arguments)], text=True, capture_output=True)
    assert (result.returncode == 0) == success, (arguments, result.stdout, result.stderr)
    checks += 1
    return result


with tempfile.TemporaryDirectory(prefix="spit-ocaml-test-") as temporary:
    folder = Path(temporary)
    pipeline = folder / "pipeline.spit"
    inventory = folder / "inventory.spitout"
    network = folder / "network"
    output = folder / "results"
    source = (
        (repository / "ocaml/examples/v1_mock.spit")
        .read_text()
        .replace("root: ../network", "root: network")
        .replace("output: ../output/v1", "output: results")
    )
    pipeline.write_text(source)
    records = []
    for site, values in [("dune", [("10", 30), ("2", 10)]), ("hill", [("2", 50), ("10", 70)])]:
        for day, value in values:
            path = network / "coast" / site / f"{day}.csv"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"time,temp\n0,{value}\n1,{value}\n")
            records.append(f"    reading[region=coast,site={site},day={day}]")
    inventory.write_text("root network\nsources:\n" + "\n".join(records) + "\n")
    run(["check", pipeline])
    original = run(["dag", pipeline, inventory]).stdout
    dag = json.loads(original)
    assert len(dag["jobs"]) == 6
    assert [job["id"] for job in dag["jobs"]] == list(range(1, 7))
    first = dag["jobs"][0]
    assert first["inputs"]["day"][0]["entities"]["day"] == "2"
    assert dag["jobs"][4]["depends_on"] == [1, 2]
    assert dag["jobs"][5]["depends_on"] == [3, 4]
    assert [a["entities"]["day"] for a in dag["jobs"][4]["inputs"]["means"]] == ["2", "10"]
    random.Random(42).shuffle(records)
    inventory.write_text("root network\nsources:\n" + "\n".join(records) + "\n")
    assert run(["dag", pipeline, inventory]).stdout == original
    saved = folder / "jobs.spitdag"
    saved.write_text(original)
    assert runner.exists(), f"Set SPIT_BASH to the existing Python runner: {runner}"
    result = subprocess.run([str(runner), "run", str(saved), "-j", "2"], text=True, capture_output=True)
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert (output / "climates/coast/dune.txt").read_text() == "20.00\n"
    assert (output / "climates/coast/hill.txt").read_text() == "60.00\n"
    result = subprocess.run([str(runner), "run", str(saved), "-j", "2"], text=True, capture_output=True)
    assert result.returncode == 0 and "6 current" in result.stdout + result.stderr, (result.stdout, result.stderr)

    simple = """root: network
output: results
default entities: {@key}={@value} separated "_"
default path: {@product}[/{@entities}]
default ext: .txt
source raw: Mean .csv [subject, run]
path raw: {subject}/{run}.csv
operation copy(x: Mean) -> Mean
command copy: cp {x} {@output}
product copied = copy(raw)
"""
    flat = "sources:\n    raw[subject=a,run=2]\n    raw[subject=a,run=10]\n"
    pipeline.write_text(simple)
    inventory.write_text(flat)
    assert len(json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"]) == 2

    failures = [
        (simple.replace("copy(raw)", "missing(raw)"), "unknown operation"),
        (simple.replace("copy(raw)", "copy(absent)"), "unknown input"),
        (simple.replace("copy(raw)", "copy(copied)"), "cycle"),
        (simple + "product copied = copy(raw)\n", "duplicate"),
        (simple.replace("copy(raw)", "copy(raw, raw)"), "number of input"),
        (simple.replace("-> Mean", "-> (one: Mean, two: Mean)"), "product names"),
        (simple.replace("x: Mean", "x: Other"), "type does not match"),
        (simple.replace("{@output}", "{absent}"), "placeholder"),
        (simple.replace("{@output}", "{x$dir}"), "output ports"),
        (simple.replace("{x} {@output}", "{x[absent]} {@output}"), "projected dimension"),
        (simple.replace("x: Mean", "x: many Mean"), "needs @vary"),
        (simple.replace("Mean .csv", "Mean / view"), "a source may be a file or folder"),
        (simple + "use: library.spit\n", "No such file"),
    ]
    for text, message in failures:
        pipeline.write_text(text)
        assert message in run(["check", pipeline], success=False).stderr

    pipeline.write_text(simple.replace("source raw: Mean", "source raw:"))
    run(["check", pipeline])
    pipeline.write_text("types: strict\n" + simple.replace("source raw: Mean", "source raw:"))
    assert "strict" in run(["check", pipeline], success=False).stderr
    generic = simple.replace("operation copy(x: Mean) -> Mean", "operation copy(x: $Kind) -> $Kind")
    pipeline.write_text(generic)
    run(["check", pipeline])
    assert (
        json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"][0]["outputs"]["output"]["type"]["name"] == "Mean"
    )
    pipeline.write_text("type Box<$A>\n" + simple.replace("source raw: Mean", "source raw: Box<Mean, Other>"))
    assert "type arguments" in run(["check", pipeline], success=False).stderr

    aggregation = simple.replace("x: Mean", "x: many Mean @min(2)").replace("copy(raw)", "copy(raw @vary(run))")
    pipeline.write_text(aggregation)
    run(["check", pipeline])
    inventory.write_text("sources:\n    raw[subject=a,run=2]\n")
    assert "@min" in run(["dag", pipeline, inventory], success=False).stderr
    inventory.write_text(flat)
    aggregated = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert len(aggregated["jobs"]) == 1
    assert aggregated["jobs"][0]["outputs"]["output"]["entities"] == {"subject": "a"}
    assert aggregated["jobs"][0]["collection"] == {"ports": [0], "varied": ["run"], "minimum": 2}
    pipeline.write_text(aggregation.replace("{x}", "--inputs={x}"))
    assert "whole argument" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(aggregation.replace("{x}", "{@join(x)}"))
    joined = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert joined["jobs"][0]["commands"][0]["command"][1][0]["join"] == ["a/2.csv", "a/10.csv"]
    pipeline.write_text(aggregation.replace("{x}", "{x[run]}"))
    projected = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert projected["jobs"][0]["commands"][0]["command"][1] == [{"port": 0, "offset": 0, "member": "2"}]

    pipeline.write_text(
        simple
        + "source cal: Mean .csv [subject]\npath cal: {subject}/cal.csv\noperation adjust(x: Mean, cal: Mean) -> Mean\ncommand adjust: cp {x} {@output}\nproduct adjusted = adjust(copied, cal)\n"
    )
    assert "missing input" in run(["dag", pipeline, inventory], success=False).stderr
    inventory.write_text(flat + "    cal[subject=a]\n")
    assert len(json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"]) == 4
    pipeline.write_text(simple + "path copied: fixed.txt\n")
    inventory.write_text(flat)
    assert "omits dimension" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(simple)
    inventory.write_text(flat + "    raw[subject=a,run=2]\n")
    assert "duplicate inventory" in run(["dag", pipeline, inventory], success=False).stderr
    inventory.write_text("sources:\n    raw[subject=a]\n")
    assert "dimensions differ" in run(["dag", pipeline, inventory], success=False).stderr

    pipeline.write_text(
        simple.replace(
            "product copied = copy(raw)",
            "stage early:\n    default ext: .tsv\n    product first = copy(raw)\nstage later:\n    product copied = copy(first)",
        )
    )
    inventory.write_text(flat)
    staged = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert staged["jobs"][0]["stage"] == ["early"]
    assert staged["jobs"][0]["outputs"]["output"]["path"].endswith(".tsv")
    assert staged["jobs"][-1]["outputs"]["output"]["path"].endswith(".txt")

    pipeline.write_text(simple + "verify copy: echo {@output}\n")
    inventory.write_text(flat)
    assert "verify" in run(["check", pipeline], success=False).stderr

    pipeline.write_text(simple.replace("x: Mean", "x: Mean, x: Mean"))
    assert "duplicate input" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(simple.replace("-> Mean", "-> (a: Mean, a: Mean)"))
    assert "duplicate input" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(simple)
    inventory.write_text(flat)
    unknown_root = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert unknown_root["root"] is None

    pipeline.write_text(simple.replace("copy(raw)", "copy(missing)"))
    location_error = run(["check", pipeline], success=False).stderr
    bad_line = next(i for i, line in enumerate(pipeline.read_text().splitlines(), 1) if "missing" in line)
    bad_column = pipeline.read_text().splitlines()[bad_line - 1].index("missing") + 1
    assert f"line {bad_line}, column {bad_column}" in location_error

    library = folder / "library.spit"
    library.write_text("""operation internal(x: Mean) -> Mean
command internal: cp {x} {@output}
operation wrapped(x: Mean) -> (result: Mean):
    product middle = internal(x)
    product result = internal(middle)
""")
    imported = simple.replace(
        "operation copy(x: Mean) -> Mean\ncommand copy: cp {x} {@output}\n", "use lib: library.spit [wrapped]\n"
    ).replace("copy(raw)", "lib.wrapped(raw)")
    pipeline.write_text(imported)
    inventory.write_text(flat)
    run(["check", pipeline])
    expanded = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert len(expanded["jobs"]) == 4
    assert expanded["jobs"][0]["outputs"]["output"]["product"] == "copied::middle"
    assert expanded["jobs"][0]["outputs"]["output"]["path"].startswith("copied.middle/")
    assert expanded["jobs"][-1]["outputs"]["output"]["product"] == "copied"
    pipeline.write_text(imported.replace("lib.wrapped(raw)", "lib.internal(raw)"))
    assert "unknown operation" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(imported.replace("[wrapped]", "[absent]"))
    assert "selected import" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(
        imported.replace("use lib: library.spit [wrapped]", "use: library.spit").replace(
            "lib.wrapped(raw)", "wrapped(raw)"
        )
    )
    run(["check", pipeline])
    library.write_text("use: pipeline.spit\n")
    pipeline.write_text(imported)
    assert "cycle" in run(["check", pipeline], success=False).stderr
    library.write_text("""operation internal(x: Mean) -> Other
command internal: cp {x} {@output}
operation wrapped(x: Mean) -> (result: Mean):
    product result = internal(x)
""")
    assert "type does not match" in run(["check", pipeline], success=False).stderr
    library.write_text("""operation wrapped(x: Mean) -> (result: Mean):
    product result = wrapped(x)
""")
    assert "cycle" in run(["check", pipeline], success=False).stderr
    library.write_text("""operation internal(x: Mean) -> Mean
command internal: cp {x} {@output}
operation wrapped(x: Mean) -> (result: Mean):
    product middle = internal(x)
""")
    assert "does not assign" in run(["check", pipeline], success=False).stderr
    library.write_text("""operation internal(x: Mean) -> Mean
command internal: cp {x} {@output}
operation wrapped(x: Mean) -> (result: Mean):
    product result = internal(x)
command wrapped: cp {x} {@output}
""")
    assert "cannot have its own command" in run(["check", pipeline], success=False).stderr
    library.write_text("""type Box<$T>
operation copy(x: Box<$T>) -> Box<$T>
command copy: cp {x} {@output}
""")
    generic_import = (
        simple.replace("source raw: Mean", "source raw: lib.Box<Mean>")
        .replace(
            "operation copy(x: Mean) -> Mean\ncommand copy: cp {x} {@output}\n", "use lib: library.spit [Box, copy]\n"
        )
        .replace("copy(raw)", "lib.copy(raw)")
    )
    pipeline.write_text(generic_import)
    run(["check", pipeline])
    typed_dag = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert typed_dag["jobs"][0]["outputs"]["output"]["type"] == {
        "name": "lib.Box",
        "args": [{"name": "Mean", "args": []}],
    }

    (folder / "libs").mkdir(exist_ok=True)
    (folder / "libs" / "commands.spit").write_text(
        "operation copy(x: Mean) -> Mean\ncommand copy: python3 {@here}/copy.py {x} {@output}\n"
    )
    pipeline.write_text(
        simple.replace(
            "operation copy(x: Mean) -> Mean\ncommand copy: cp {x} {@output}\n", "use lib: libs/commands.spit [copy]\n"
        ).replace("copy(raw)", "lib.copy(raw)")
    )
    inventory.write_text(flat)
    imported_here = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert imported_here["jobs"][0]["commands"][0]["command"][1] == [{"here": "libs"}, "/copy.py"]

    for selector, expected_run in [("@first(run)", "2"), ("@last(run)", "10"), ("@where(run=2)", "2")]:
        pipeline.write_text(simple.replace("copy(raw)", f"copy(raw {selector})"))
        inventory.write_text(flat)
        selected = json.loads(run(["dag", pipeline, inventory]).stdout)
        assert len(selected["jobs"]) == 1
        assert selected["jobs"][0]["inputs"]["x"][0]["entities"]["run"] == expected_run
        assert selected["jobs"][0]["outputs"]["output"]["entities"] == {"subject": "a"}
    pipeline.write_text(simple.replace("copy(raw)", "copy(raw @same(subject))"))
    assert "ambiguous" in run(["dag", pipeline, inventory], success=False).stderr
    pipeline.write_text(aggregation.replace("@vary(run)", "@where(subject=a) @vary(run)"))
    collected = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert collected["jobs"][0]["outputs"]["output"]["entities"] == {}
    broadcast = simple.replace(
        "operation copy(x: Mean) -> Mean",
        "source alert: Mean .txt [alert, scenario]\npath alert: alerts/{alert}/{scenario}.txt\noperation copy(x: Mean, alert: Mean) -> Mean",
    ).replace("copy(raw)", "copy(raw, alert @each(alert, scenario))")
    pipeline.write_text(broadcast)
    inventory.write_text(flat + "    alert[alert=low,scenario=hot]\n    alert[alert=high,scenario=cold]\n")
    broadcasted = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert len(broadcasted["jobs"]) == 4
    assert {
        (j["outputs"]["output"]["entities"]["alert"], j["outputs"]["output"]["entities"]["scenario"])
        for j in broadcasted["jobs"]
    } == {("low", "hot"), ("high", "cold")}
    inventory.write_text(flat)
    assert json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"] == []

    execution = """root: network
output: numeric-results
default path: {@product}
default ext: .txt
source raw: Mean .txt []
path raw: number.txt
operation numeric(x: Mean) -> Mean
calculate numeric: round(-x ^ 2 + 10, 2)
resources numeric: cores=2, memory=64M
anchored numeric
product answer = numeric(raw)
"""
    (network / "number.txt").write_text("2.5\n")
    inventory.write_text("root network\nsources:\n    raw[]\n")
    pipeline.write_text(execution)
    numeric_dag = json.loads(run(["dag", pipeline, inventory]).stdout)
    numeric_job = numeric_dag["jobs"][0]
    assert (numeric_job["cores"], numeric_job["memory"], numeric_job["anchored"]) == (2, 67108864, True)
    assert numeric_job["commands"][0]["command"] == [[{"calculate": ["round", ["+", ["neg", ["^", "x", 2]], 10], 2]}]]
    saved.write_text(json.dumps(numeric_dag))
    result = subprocess.run([str(runner), "run", str(saved), "-j", "2"], text=True, capture_output=True)
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert float((folder / "numeric-results/answer.txt").read_text()) == 3.75
    scalar = execution.replace(
        "calculate numeric: round(-x ^ 2 + 10, 2)",
        "command numeric: printf '%s' {@scalar(x, min=0, max=3)} > {@output}",
    )
    pipeline.write_text(scalar)
    scalar_dag = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert scalar_dag["jobs"][0]["commands"][0]["command"][2] == [{"scalar": "number.txt", "min": 0, "max": 3}]
    saved.write_text(json.dumps(scalar_dag))
    result = subprocess.run([str(runner), "run", str(saved), "-j", "2"], text=True, capture_output=True)
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert float((folder / "numeric-results/answer.txt").read_text()) == 2.5
    (network / "number.txt").write_text("4\n")
    result = subprocess.run([str(runner), "run", str(saved), "-j", "2"], text=True, capture_output=True)
    assert result.returncode != 0, (result.stdout, result.stderr)
    inherited = (
        scalar.replace(
            "resources numeric: cores=2, memory=64M",
            "default resources: cores=3, memory=1G\nresources numeric: cores=2",
        )
        .replace("{@scalar(x, min=0, max=3)}", "{@cores}")
        .replace("anchored numeric", "default anchored")
    )
    pipeline.write_text(inherited)
    inherited_job = json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"][0]
    assert (inherited_job["cores"], inherited_job["memory"], inherited_job["anchored"]) == (2, 1073741824, True)
    assert inherited_job["commands"][0]["command"][2] == ["2"]
    for before, after, message in [
        ("cores=2", "cores=0", "positive integer"),
        ("memory=64M", "memory=1Z", "memory unit"),
        ("memory=64M", "memory=999999999999T", "2^53"),
        ("cores=2", "cores=2, cores=3", "repeated"),
        ("round(-x ^ 2 + 10, 2)", "unknown + 1", "input port"),
        ("round(-x ^ 2 + 10, 2)", "sqrt(x, 2)", "argument count"),
        ("round(-x ^ 2 + 10, 2)", "1e999", "finite"),
        ("round(-x ^ 2 + 10, 2)", "x +", "expected"),
        ("round(-x ^ 2 + 10, 2)", "(" * 70 + "x" + ")" * 70, "64 levels"),
        ("round(-x ^ 2 + 10, 2)", " + ".join(["x"] * 70), "128 nodes"),
    ]:
        pipeline.write_text(execution.replace(before, after))
        assert message in run(["check", pipeline], success=False).stderr
    for expression in [
        "{@scalar(x, min=4, max=3)}",
        "{@scalar(x, min=0, min=1)}",
        "{@scalar(x, max=nan)}",
        "{@scalar(x, lower=0)}",
    ]:
        pipeline.write_text(scalar.replace("{@scalar(x, min=0, max=3)}", expression))
        run(["check", pipeline], success=False)
    pipeline.write_text(execution + "command numeric: cp {x} {@output}\n")
    assert "conflicts" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(execution + "resources numeric: cores=1\n")
    assert "repeated" in run(["check", pipeline], success=False).stderr

    library.write_text(
        "operation numeric(x: Mean) -> Mean\ncalculate numeric: x + 1\nresources numeric: cores=4, memory=32M\nanchored numeric\n"
    )
    pipeline.write_text(
        execution.replace(
            "operation numeric(x: Mean) -> Mean\ncalculate numeric: round(-x ^ 2 + 10, 2)\nresources numeric: cores=2, memory=64M\nanchored numeric\n",
            "default resources: cores=7\nuse lib: library.spit [numeric]\n",
        ).replace("numeric(raw)", "lib.numeric(raw)")
    )
    imported_config = json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"][0]
    assert (imported_config["cores"], imported_config["memory"], imported_config["anchored"]) == (4, 33554432, True)

    pipeline.write_text(
        simple.replace("command copy: cp {x} {@output}", r"command copy: printf %s \> \| \&\& '>' > {@output}")
    )
    inventory.write_text(flat)
    escaped = json.loads(run(["dag", pipeline, inventory]).stdout)["jobs"][0]["commands"][0]
    assert escaped["command"][2:] == [[">"], ["|"], ["&&"], [">"]]
    assert escaped["stdout"].endswith(".txt")
    for operator in ["|", "||", "&&", ";", ">>", "&"]:
        pipeline.write_text(simple.replace("command copy: cp {x} {@output}", f"command copy: echo {operator}"))
        assert "explicit shell" in run(["check", pipeline], success=False).stderr
    pipeline.write_text(
        simple.replace("path raw: {subject}/{run}.csv", "path raw: [prefix/{missing}/]{subject}/{run}.csv")
    )
    optional = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert optional["jobs"][0]["inputs"]["x"][0]["path"] == "a/2.csv"
    pipeline.write_text(
        simple.replace(
            'default entities: {@key}={@value} separated "_"',
            'default entities: {@key}={@value} separated "_"\nentities subject: sub',
        )
    )
    renamed = json.loads(run(["dag", pipeline, inventory]).stdout)
    assert "sub=a" in renamed["jobs"][0]["outputs"]["output"]["path"]
    for invalid_pattern, message in [
        ("path unused: {@unknown}", "unknown built-in placeholder"),
        ('default entities: {@stage} separated "_"', "no other placeholders"),
        ("default entities: {@key}", "separated"),
    ]:
        pipeline.write_text(simple + invalid_pattern + "\n")
        assert message in run(["check", pipeline], success=False).stderr
    pipeline.write_text(simple.replace("path raw: {subject}/{run}.csv", "path raw: {missing}/{run}.csv"))
    missing = run(["check", pipeline], success=False).stderr
    assert "line 7, column 12" in missing and "absent dimension `missing`" in missing, missing

    durations = []
    for count in [1000, 4000]:
        pipeline.write_text(simple)
        inventory.write_text("sources:\n" + "".join(f"    raw[subject=a,run={i}]\n" for i in range(count)))
        start = time.perf_counter()
        scaled = json.loads(run(["dag", pipeline, inventory]).stdout)
        elapsed = time.perf_counter() - start
        assert len(scaled["jobs"]) == count
        durations.append((count, elapsed))
    assert durations[1][1] < durations[0][1] * 8 + 0.2, durations
    print(
        f"Compiler checks passed: {checks} CLI cases; runner verified weather jobs, reuse, calculations, scalar reads and bounds."
    )
    print("Inventory scaling: " + ", ".join(f"{count}: {elapsed:.3f}s" for count, elapsed in durations))
