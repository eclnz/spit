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
    assert [item["entities"]["run"] for item in document["external_inputs"]] == ["2", "10"]
    output = folder / "study.spitdag"
    assert run("dag", recipe, "-o", output).stdout == ""
    assert json.loads(output.read_text()) == document

    pipeline.write_text(pipeline.read_text().replace("path raw: {subject}/{run}.csv\n", ""))
    recipe.write_text("pipeline pipelines/analysis.spit\nroot data\ninputs:\n    raw: {subject}/{run}.csv\n")
    assert json.loads(run("dag", recipe).stdout) == document
    pipeline.write_text("root: ../data\n" + pipeline.read_text())
    recipe.write_text(recipe.read_text().replace("root data\n", ""))
    assert json.loads(run("dag", recipe).stdout) == document
    recipe.write_text(recipe.read_text().replace("inputs:", "root data\ninputs:"))
    assert "both" in run("dag", recipe, success=False).stderr
    pipeline.write_text(pipeline.read_text().replace("root: ../data\n", ""))
    recipe.write_text(recipe.read_text().replace("raw:", "absent:"))
    assert "source" in run("dag", recipe, success=False).stderr
    recipe.write_text("pipeline pipelines/analysis.spit\nroot data\nroot data\n")
    assert "twice" in run("dag", recipe, success=False).stderr
    recipe.write_text("root data\n")
    assert "pipeline" in run("dag", recipe, success=False).stderr
    recipe.write_text("pipeline pipelines/analysis.spit\nroot absent\n")
    run("dag", recipe, success=False)

print("Recipe loading passed: relative paths, discovery, overrides, output and invalid recipes.")
