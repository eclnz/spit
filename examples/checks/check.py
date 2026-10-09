#!/usr/bin/env python3
"""Exercise every example through the freshly built CLI, without running jobs."""

import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True

from graph import concepts, require, validate

REPO = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("coverage.json")


def coverage(repo, manifest):
    found = {str(p.relative_to(repo)) for p in (repo / "examples").rglob("*")
             if p.suffix in {".spit", ".spitin"}}
    classified = [p for e in manifest["entries"] for p in [e["pipeline"], e["recipe"]]]
    classified += list(manifest["libraries"]) + list(manifest["invalid"])
    require(len(classified) == len(set(classified)), "a file is classified more than once")
    require(found == set(classified),
            f"coverage differs: unclassified={sorted(found - set(classified))}; stale={sorted(set(classified) - found)}")
    entries = {e["pipeline"] for e in manifest["entries"]}
    for library, callers in manifest["libraries"].items():
        require(bool(callers) and set(callers) <= entries, f"library {library} needs classified callers")


class Run:
    def __init__(self, binary, workspace, logs):
        self.binary = binary
        self.workspace = workspace
        self.logs = logs
        self.phase = "setup"

    def cli(self, phase, args, success=True, contains=None):
        self.phase = phase
        output = subprocess.run([str(self.binary), *map(str, args)], cwd=self.workspace,
                                capture_output=True, text=True, timeout=60)
        self.logs.mkdir(parents=True, exist_ok=True)
        (self.logs / f"{phase}.log").write_text(
            f"$ spit {' '.join(map(str, args))}\nexit: {output.returncode}\n"
            f"--- stdout\n{output.stdout}\n--- stderr\n{output.stderr}")
        require((output.returncode == 0) == success,
                f"{phase}: unexpected exit {output.returncode}: {output.stderr}")
        if contains is not None:
            require(contains in output.stdout + output.stderr, f"{phase}: missing diagnostic {contains!r}")
        return output.stdout


def workspace(repo, target):
    # Copy actual example data and imports, excluding every stored inventory.
    shutil.copytree(repo / "examples", target / "examples",
                    ignore=shutil.ignore_patterns("*.spitout", "*.spitdag", "checks", "__pycache__"))


def check_entry(binary, repo, entry, logs):
    with tempfile.TemporaryDirectory(prefix="spit-example-") as temp:
        work = Path(temp)
        workspace(repo, work)
        run = Run(binary, work, logs)
        pipeline, recipe = entry["pipeline"], entry["recipe"]
        try:
            for phase, file in [("check-pipeline", pipeline), ("check-recipe", recipe)]:
                checked = json.loads(run.cli(phase, ["check", file, "--json"]))
                require(checked["diagnostics"] == [], f"{phase}: unexpected diagnostics")
            inventory = logs / "fresh.spitout"
            run.cli("discover", ["inputs", recipe, "-o", inventory])
            require(inventory.is_file(), "discovery wrote no inventory")
            require(inventory.stat().st_size > 0, "empty inventory")
            checked = json.loads(run.cli("check-inventory", ["check", inventory, "--json"]))
            require(checked["diagnostics"] == [], "fresh inventory has diagnostics")
            direct = json.loads(run.cli("direct-recipe", ["dag", recipe, "--json"]))
            fresh = json.loads(run.cli("fresh-inventory", ["dag", pipeline, inventory, "--json"]))
            # Saved inventories express the root relative to their own location.
            # Canonicalize only that representation; compare every other field exactly.
            direct["root"] = str(Path(direct["root"]).resolve())
            fresh["root"] = str(Path(fresh["root"]).resolve())
            require(direct == fresh, "direct recipe and fresh inventory resolution differ")
            saved = logs / "jobs.spitdag"
            run.cli("save-dag", ["dag", pipeline, inventory, "-o", saved])
            dag = json.loads(saved.read_text())
            dag["root"] = str(Path(dag["root"]).resolve())
            require(dag == fresh, "saved DAG and JSON resolution differ")
            run.phase = "graph"
            validate(dag, entry["expect"])
            concepts(Path(pipeline).stem, dag)
            return dag
        except Exception as error:
            raise AssertionError(f"{pipeline} [{run.phase}]: {error}") from error


def check_invalid(binary, repo, file, expected, logs):
    run = Run(binary, repo, logs)
    checked = json.loads(run.cli("check-invalid", ["check", file, "--json"], success=False))
    errors = [d for d in checked["diagnostics"] if d["severity"] == "error"]
    require(len(errors) == len(checked["diagnostics"]) == 1, f"{file}: expected one error, got {errors}")
    require(errors[0]["message"] == expected["message"],
            f"{file}: unintended diagnostic {errors}")


def caught(action, message):
    try:
        action()
    except AssertionError:
        return
    raise AssertionError(f"negative probe was not detected: {message}")


def probes(binary, repo, manifest, logs):
    """Mutate only disposable copies and prove both CLI and assertion failures."""
    selector = next(e for e in manifest["entries"] if Path(e["pipeline"]).stem == "selectors")
    with tempfile.TemporaryDirectory(prefix="spit-probes-") as temp:
        work = Path(temp)
        workspace(repo, work)
        run = Run(binary, work, logs)
        pipeline = work / selector["pipeline"]
        original = pipeline.read_text()
        pipeline.write_text(original + "\nstage broken\n")
        run.cli("syntax", ["check", selector["pipeline"], "--json"], success=False, contains="stage name:")
        pipeline.write_text(original)
        recipe = work / selector["recipe"]
        original_recipe = recipe.read_text()
        recipe.write_text(original_recipe + "\nrequire [station] where reading count>=99\n")
        run.cli("recipe", ["inputs", selector["recipe"]], success=False, contains="99")
        recipe.write_text(original_recipe)
        missing = work / "examples/pipelines/selectors_data/policy/north.toml"
        missing.unlink()
        run.cli("missing-source", ["inputs", selector["recipe"]], success=False, contains="policy")
        missing.touch()
        pipeline.write_text(original.replace("where(revision=2)", "where(revision=1)"))
        # This is syntactically valid but leaves south without its calibration.
        run.cli("missing-join", ["dag", selector["recipe"], "--json"], success=False, contains="calibration")
        pipeline.write_text(original)
        dag = json.loads(run.cli("valid-graph", ["dag", selector["recipe"], "--json"]))
        validate(dag, selector["expect"])
        concepts("selectors", dag)
        pipeline.write_text(original.replace("--low {low} --high {high}", "--low {high} --high {low}"))
        changed = json.loads(run.cli("wrong-binding-cli-succeeds", ["dag", selector["recipe"], "--json"]))
        caught(lambda: concepts("selectors", changed), "valid but wrong command binding")
        pipeline.write_text(original.replace("where(revision=2)", "where(revision=1)"))
        # Supply a south revision 1 too, so generation now succeeds with the wrong selection.
        (work / "examples/pipelines/selectors_data/calibration/south/r1.json").touch()
        changed = json.loads(run.cli("wrong-selection-cli-succeeds", ["dag", selector["recipe"], "--json"]))
        caught(lambda: concepts("selectors", changed), "valid but wrong selector")
        broken = copy.deepcopy(dag)
        job = next(j for j in broken["jobs"] if j["depends_on"])
        job["depends_on"] = []
        # Preserve total edge count to test producer validation, not just counts.
        other = next(j for j in broken["jobs"] if j["id"] != job["id"])
        other["depends_on"] += next(j for j in dag["jobs"] if j["id"] == job["id"])["depends_on"]
        caught(lambda: validate(broken, selector["expect"]), "wrong producer edges with unchanged count")
        wrong_counts = copy.deepcopy(selector["expect"])
        wrong_counts["dependencies"] += 1
        caught(lambda: validate(dag, wrong_counts), "wrong expectation despite successful CLI")
        bad = copy.deepcopy(manifest)
        bad["entries"] = bad["entries"][1:]
        caught(lambda: coverage(repo, bad), "unclassified files")
        bad = copy.deepcopy(manifest)
        bad["libraries"]["examples/stale.spit"] = [selector["pipeline"]]
        caught(lambda: coverage(repo, bad), "stale coverage")
    (logs / "assertion-probes.log").write_text(
        "Detected wrong command binding, selector, producer edges, expected counts, unclassified and stale entries.\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=REPO / "target/release/spit")
    parser.add_argument("--logs", type=Path, help="empty artifact directory (default: a new run under target/example-checks)")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.logs is None:
        parent = REPO / "target/example-checks"
        parent.mkdir(parents=True, exist_ok=True)
        logs = Path(tempfile.mkdtemp(prefix="run-", dir=parent)).resolve()
    else:
        logs = args.logs.resolve()
    require(binary.is_file(), f"build the binary first: cargo build --release ({binary})")
    require(not logs.exists() or not any(logs.iterdir()), f"use an empty logs directory: {logs}")
    logs.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(MANIFEST.read_text())
    coverage(REPO, manifest)
    failures, seen = [], {}
    for entry in manifest["entries"]:
        name = str(Path(entry["pipeline"]).with_suffix("")).replace("/", "__")
        try:
            dag = check_entry(binary, REPO, entry, logs / name)
            seen[entry["pipeline"]] = {
                str(Path(entry["pipeline"]).parent / f["path"]) for f in dag["pipeline_files"]
            }
            print(f"ok {entry['pipeline']}: check, discovery, inventory, DAG, graph", flush=True)
        except Exception as error:
            failures.append(str(error))
            print(f"FAIL {error}", file=sys.stderr, flush=True)
    for file, expected in manifest["invalid"].items():
        try:
            check_invalid(binary, REPO, file, expected, logs / Path(file).stem)
            print(f"ok {file}: intended diagnostic", flush=True)
        except Exception as error:
            failures.append(str(error))
    for library, callers in manifest["libraries"].items():
        try:
            require(all(library in seen.get(caller, set()) for caller in callers),
                    f"library {library} was not covered through {callers}")
            print(f"ok {library}: imported by callers", flush=True)
        except Exception as error:
            failures.append(str(error))
    try:
        probes(binary, REPO, manifest, logs / "negative-probes")
        print("ok negative probes: syntax, recipe, discovery, joins, graph assertions, classification", flush=True)
    except Exception as error:
        failures.append(f"negative probes: {error}")
    (logs / "summary.json").write_text(json.dumps({"failures": failures}, indent=2) + "\n")
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    print(f"Logs and plans: {logs}", flush=True)
    return bool(failures)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"FAIL setup: {error}", file=sys.stderr)
        sys.exit(1)
