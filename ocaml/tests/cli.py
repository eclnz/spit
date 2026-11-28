import json
import subprocess
import sys
import tempfile
from pathlib import Path

compiler = Path(sys.argv[1]).resolve()
repository = Path(sys.argv[2]).resolve()


def run(*arguments, success=True):
    result = subprocess.run([str(compiler), *map(str, arguments)], capture_output=True, text=True, check=False)
    assert (result.returncode == 0) == success, (arguments, result.stdout, result.stderr)
    return result


version = (repository / "VERSION").read_text().strip()
assert run("--version").stdout == f"spit {version}\n"
assert "spit dag" in run("--help").stdout
run("absent", success=False)
run("check", "missing.spit", success=False)
run("check", "missing.spit", "--json", success=False)

with tempfile.TemporaryDirectory(prefix="spit-cli-") as temporary:
    folder = Path(temporary).resolve()
    pipeline = folder / "pipeline.spit"
    pipeline.write_text((repository / "ocaml/tests/parity/ocaml.spit").read_text())
    inventory = folder / "inputs.spitout"
    inventory.write_text("root data\nsources:\n    raw[subject=a,run=2]\n    raw[subject=a,run=10]\n")
    document = run("dag", pipeline, inventory).stdout
    parsed = json.loads(document)
    assert parsed["version"] == 18
    assert parsed["generator"] == {"name": "spit", "version": version}
    assert run("dag", pipeline, inventory, "--json", "-o", "-").stdout == document
    for option in ("-o", "--output"):
        output = folder / "result.spitdag"
        assert run("dag", pipeline, inventory, option, output).stdout == ""
        assert output.read_text() == document
    output.write_text("keep this file")
    run("dag", pipeline, folder / "absent", "-o", output, success=False)
    assert output.read_text() == "keep this file"
    run("dag", pipeline, inventory, "-o", folder, success=False)
    run("dag", pipeline, inventory, "-o", success=False)
    run("dag", pipeline, inventory, "--unknown", success=False)
    run("dag", pipeline, inventory, "-o", output, "-o", output, success=False)

print("CLI checks passed: version, help, file/stdout DAG output, v18 schema and failure handling.")

with tempfile.TemporaryDirectory(prefix="spit-recipe-") as temporary:
    folder = Path(temporary).resolve()
    library = folder / "pipelines"
    library.mkdir()
    dataset = folder / "data"
    (dataset / "alice").mkdir(parents=True)
    (dataset / "alice/2.csv").write_text("two\n")
    (dataset / "alice/10.csv").write_text("ten\n")
    pipeline = library / "analysis.spit"
    pipeline.write_text((repository / "ocaml/tests/parity/ocaml.spit").read_text())
    recipe = folder / "study.spitin"
    recipe.write_text("pipeline pipelines/analysis.spit\nroot data\n")
    document = json.loads(run("dag", recipe, "--json").stdout)
    assert document["root"] == str(dataset)
    assert document["output_root"] == str(library / "results")
    assert len(document["jobs"]) == 3
    assert json.loads(run("dag", pipeline, "--root", dataset, "--json").stdout) == document
    run("dag", pipeline, success=False)
    run("dag", pipeline, "--root", dataset, "--root", dataset, success=False)
    assert [item["entities"]["run"] for item in document["external_inputs"]] == ["2", "10"]
    output = folder / "study.spitdag"
    assert run("dag", recipe, "-o", output).stdout == ""
    assert json.loads(output.read_text()) == document
    pipeline.write_text(
        pipeline.read_text().replace("product copied =", "copied =").replace("product aggregate =", "aggregate =")
    )
    assert json.loads(run("dag", recipe).stdout) == document

    original_pipeline = pipeline.read_text()
    pipeline.write_text("root ../data\n" + original_pipeline)
    assert json.loads(run("dag", pipeline).stdout) == document
    assert "both" in run("dag", pipeline, "--root", dataset, success=False).stderr
    pipeline.write_text(original_pipeline)

    pipeline.write_text(pipeline.read_text().replace("path raw: {subject}/{run}.csv\n", ""))
    recipe.write_text("pipeline pipelines/analysis.spit\nroot data\ninputs:\n    raw: {subject}/{run}.csv\n")
    assert json.loads(run("dag", recipe).stdout) == document
    pipeline.write_text("root ../data\n" + pipeline.read_text())
    recipe.write_text(recipe.read_text().replace("root data\n", ""))
    assert json.loads(run("dag", recipe).stdout) == document
    recipe.write_text(recipe.read_text().replace("inputs:", "root data\ninputs:"))
    assert "both" in run("dag", recipe, success=False).stderr
    pipeline.write_text(pipeline.read_text().replace("root ../data\n", ""))
    recipe.write_text(recipe.read_text().replace("raw:", "absent:"))
    assert "source" in run("dag", recipe, success=False).stderr
    recipe.write_text("pipeline pipelines/analysis.spit\nroot data\nroot data\n")
    assert "twice" in run("dag", recipe, success=False).stderr
    recipe.write_text("root data\n")
    assert "pipeline" in run("dag", recipe, success=False).stderr
    recipe.write_text("pipeline pipelines/analysis.spit\nroot absent\n")
    run("dag", recipe, success=False)

print("Recipe loading passed: relative paths, discovery, overrides, output and invalid recipes.")

with tempfile.TemporaryDirectory(prefix="spit-checks-") as temporary:
    folder = Path(temporary)
    (folder / "data").mkdir()
    (folder / "data/one.txt").write_text("one\n")
    pipeline = folder / "checks.spit"
    pipeline.write_text("""check nonempty: test -s {@path}
check contains(word): grep -q {word} {@path}
source raw : Text [id]
path raw: data/{id}.txt
default path: out/{@product}/{id}.txt
default check: nonempty
operation copy(input: Text) -> Text @check(!nonempty)
command copy: cp {input} {@output}
stage checked:
    operation inspect(input: Text @check(contains(one))) -> Text
    command inspect: cp {input} {@output}
    result = inspect(raw)
plain = copy(raw)
""")
    recipe = folder / "checks.spitin"
    recipe.write_text("pipeline checks.spit\nroot .\n")
    document = json.loads(run("dag", recipe).stdout)
    jobs = {job["operation"]: job for job in document["jobs"]}
    assert jobs["copy"]["checks"] == []
    checks = jobs["inspect"]["checks"]
    assert [(check["when"], check["check"]) for check in checks] == [
        ("before", "contains(one)"),
        ("after", "nonempty"),
    ]
    assert {"test", "grep"}.issubset(document["executables"])
    original = pipeline.read_text()
    for wrong, message in [
        ("contains(one, two)", "wrong number"),
        ("absent(one)", "unknown check"),
    ]:
        pipeline.write_text(original.replace("contains(one)", wrong))
        assert message in run("check", pipeline, success=False).stderr

print("Artifact checks passed: parameters, defaults, opt-outs, stage inputs and diagnostics.")

with tempfile.TemporaryDirectory(prefix="spit-tools-") as temporary:
    folder = Path(temporary)
    (folder / "data").mkdir()
    (folder / "script.py").write_text("print('hello')\n")
    pipeline = folder / "pipeline.spit"
    pipeline.write_text("""tool python
source script : Script .py here
path script: script.py
operation run(script: Script) -> Text
uses run: python
command run: python3 {script} > {@output}
result = run(script)
path result: output.txt
""")
    recipe = folder / "recipe.spitin"
    recipe.write_text("pipeline pipeline.spit\nroot data\ntools from runtime.spitenv\n")
    environment = folder / "runtime.spitenv"
    environment.write_text("tools:\n    python: probe python3 --version\n")
    document = json.loads(run("dag", recipe).stdout)
    assert document["external_inputs"][0]["under"] == "pipeline_folder"
    assert document["jobs"][0]["tools"] == ["python"]
    assert document["tools"][0]["probe"] == ["python3", "--version"]
    environment.write_text("tools:\n")
    assert "no identity" in run("dag", recipe, success=False).stderr
    environment.write_text("tools:\n    python: python-3\n")
    assert json.loads(run("dag", recipe).stdout)["tools"][0]["identity"] == "python-3"
    (folder / "script.py").unlink()
    assert "pipeline-local source file is missing" in run("dag", recipe, success=False).stderr
    (folder / "script.py").write_text("print('hello')\n")
    pipeline.write_text(pipeline.read_text().replace("uses run: python", "uses run: unknown"))
    assert "undeclared tool" in run("check", pipeline, success=False).stderr

print("Tools and here sources passed: identities, probes, source roots and missing tools.")
