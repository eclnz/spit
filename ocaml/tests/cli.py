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
