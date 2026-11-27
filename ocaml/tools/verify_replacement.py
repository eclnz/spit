import argparse
import copy
import json
import os
import subprocess
import tempfile
from pathlib import Path


def execute(arguments, valid=True):
    result = subprocess.run(list(map(str, arguments)), capture_output=True, text=True, check=False)
    if (result.returncode == 0) != valid:
        raise AssertionError((arguments, result.returncode, result.stdout, result.stderr))
    return result.stdout


def planned_work(document):
    result = copy.deepcopy(document)
    provenance = {key: result.pop(key) for key in ("generator", "pipeline_files", "calls")}
    provenance["origins"] = [job.pop("origin") for job in result["jobs"]]
    return result, provenance


def paired_checks(compilers, paths, reference, candidate):
    for before, after in [
        ("copy(raw)", "absent(raw)"),
        ("copy(raw)", "copy(missing)"),
        ("copy(raw)", "copy(copied)"),
        ("input: Mean", "input: Other"),
    ]:
        paths[0].write_text(reference.replace(before, after))
        paths[1].write_text(candidate.replace(before, after))
        for compiler, path in zip(compilers, paths):
            execute([compiler, "check", path], valid=False)
    paths[0].write_text(reference)
    paths[1].write_text(candidate)


def compare_dags(compilers, paths, inventory):
    outputs = [
        execute([compilers[0], "dag", paths[0], inventory, "--json"]),
        execute([compilers[1], "dag", paths[1], inventory]),
    ]
    documents = list(map(json.loads, outputs))
    assert all(document["version"] == 18 for document in documents)
    work = [planned_work(document) for document in documents]
    if work[0][0] != work[1][0]:
        folder = Path(tempfile.mkdtemp(prefix="spit-parity-difference-"))
        for name, document in zip(("reference", "ocaml"), documents):
            (folder / (name + ".json")).write_text(json.dumps(document, indent=2) + "\n")
        raise AssertionError(f"Version-18 planned work differs; documents saved under {folder}")
    fields = sorted(key for key in work[0][1] if work[0][1][key] != work[1][1][key])
    print("Metadata differences: " + ", ".join(fields))
    return outputs


def verify(rust, ocaml, runner, fixtures):
    with tempfile.TemporaryDirectory(prefix="spit-replacement-") as directory:
        root = Path(directory).resolve()
        paths = [root / "reference.spit", root / "ocaml.spit"]
        texts = [(fixtures / name).read_text() for name in ("reference.spit", "ocaml.spit")]
        data = root / "data"
        for subject in ("a", "b"):
            (data / subject).mkdir(parents=True)
            for run in ("2", "10"):
                (data / subject / f"{run}.csv").write_text(f"{subject}:{run}\n")
        records = [f"    raw[subject={subject},run={run}]\n" for subject in ("a", "b") for run in ("2", "10")]
        inventory = root / "inputs.spitout"
        inventory.write_text("root data\nsources:\n" + "".join(records))
        compilers = [rust, ocaml]
        paired_checks(compilers, paths, *texts)
        for compiler, path in zip(compilers, paths):
            execute([compiler, "check", path])
        outputs = compare_dags(compilers, paths, inventory)
        inventory.write_text("root data\nsources:\n" + "".join(reversed(records)))
        assert compare_dags(compilers, paths, inventory) == outputs, "Inventory order changes output"
        inventory.write_text("root data\nsources:\n" + "".join(records[:1]))
        execute([rust, "dag", paths[0], inventory, "--json"], valid=False)
        execute([ocaml, "dag", paths[1], inventory], valid=False)
        if runner:
            saved = root / "jobs.spitdag"
            saved.write_text(outputs[0])
            execute([runner, "run", saved, "-j", "2"])
            assert (root / "results/aggregate/subject=a.txt").read_text() == "a:2\na:10\n"
            assert (root / "results/aggregate/subject=b.txt").read_text() == "b:2\nb:10\n"
            saved.write_text(outputs[1])
            result = subprocess.run(
                [str(runner), "run", str(saved), "-j", "2"], capture_output=True, text=True, check=False
            )
            assert result.returncode == 0 and "6 current" in result.stdout + result.stderr, (
                result.stdout,
                result.stderr,
            )
    print(
        "Existing-contract checks passed: planned work matches at v18; shuffled inventories, four invalid pipelines, collection minimum, execution and cross-compiler reuse checked."
    )
    print("Provenance and generator differences remain explicit; this fixture is not full replacement certification.")


def main():
    parser = argparse.ArgumentParser()
    repository = Path(__file__).resolve().parents[2]
    parser.add_argument("--rust", type=Path, default=repository / "target/debug/spit")
    parser.add_argument("--ocaml", type=Path, default=repository / "ocaml/spit.sh")
    parser.add_argument(
        "--runner", type=Path, default=Path(os.environ.get("SPIT_BASH", repository / ".venv/bin/spit-bash"))
    )
    args = parser.parse_args()
    verify(args.rust.resolve(), args.ocaml.resolve(), args.runner.resolve(), repository / "ocaml/tests/parity")


if __name__ == "__main__":
    main()
