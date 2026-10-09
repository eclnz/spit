"""Independent checks of the saved compiler plan; never execute commands."""

from collections import Counter
from pathlib import Path


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def identity(artifact):
    return artifact["product"], tuple(sorted(artifact["entities"].items()))


def summary(dag):
    return {
        "sources": dict(sorted(Counter(a["product"] for a in dag["external_inputs"]).items())),
        "operations": dict(sorted(Counter(j["operation"] for j in dag["jobs"]).items())),
        "targets": dict(sorted(Counter(a["product"] for a in dag["targets"]).items())),
        "dependencies": sum(len(j["depends_on"]) for j in dag["jobs"]),
        "removals": len(dag["removed"]),
        "calls": len(dag["calls"]),
    }


def commands(job):
    return ([job["command"]] if job["command"] is not None else []) + job["verify"] + [
        check["command"] for check in job["checks"]
    ]


def validate(dag, expected):
    require(not dag["left_out"], "complete examples must leave no output out")
    require(summary(dag) == expected, f"graph counts differ: {summary(dag)} != {expected}")
    jobs = dag["jobs"]
    by_id = {j["id"]: j for j in jobs}
    require(list(by_id) == list(range(1, len(jobs) + 1)), "job ids must be unique and sequential")
    produced, paths = {}, set()
    for job in jobs:
        for artifact in job["outputs"].values():
            key = identity(artifact)
            require(key not in produced, f"two producers of {key}")
            require(artifact["path"] not in paths, f"two outputs at {artifact['path']}")
            produced[key] = (job["id"], artifact)
            paths.add(artifact["path"])

    read, external = set(), {}
    for job in jobs:
        dependencies = set()
        bound = {a["path"] for a in job["outputs"].values()}
        for artifacts in job["inputs"].values():
            for artifact in artifacts:
                key = identity(artifact)
                read.add(key)
                bound.add(artifact["path"])
                if key in produced:
                    producer, output = produced[key]
                    require(output == artifact, f"input differs from its producer: {key}")
                    require(producer < job["id"], f"non-topological dependency: {key}")
                    dependencies.add(producer)
                else:
                    require(key not in external or external[key] == artifact, f"source differs: {key}")
                    external[key] = artifact
                    source = Path(dag["root"]) / artifact["path"]
                    require(source.is_dir() if artifact["kind"] == "folder" else source.is_file(),
                            f"discovered source does not exist: {source}")
        require(job["depends_on"] == sorted(dependencies), f"wrong producers for job {job['id']}")
        dependents = [j["id"] for j in jobs if job["id"] in j["depends_on"]]
        require(job["dependents"] == dependents, f"wrong dependents for job {job['id']}")
        for command in commands(job):
            require(bool(command), f"empty command in job {job['id']}")
            for argument in command:
                require(bool(argument), f"empty argument in job {job['id']}")
                for part in argument:
                    if isinstance(part, dict):
                        path = part.get("path", part.get("of"))
                        require(path in bound, f"command path is unbound in job {job['id']}: {path}")
                        if "dir" in part:
                            require(part["dir"] == str(Path(path).parent), "wrong directory argument")
        origin = job["origin"]
        if origin is not None:
            require(0 <= origin["call"] < len(dag["calls"]), "unknown call origin")
            require(origin["line"] > 0, "invalid body line")

    require(len(dag["external_inputs"]) == len(external), "duplicate or missing external inputs")
    require({identity(a): a for a in dag["external_inputs"]} == external, "wrong external inputs")
    targets = {key: artifact for key, (_, artifact) in produced.items() if key not in read}
    require(len(dag["targets"]) == len(targets), "duplicate or missing targets")
    require({identity(a): a for a in dag["targets"]} == targets, "targets are not terminal outputs")


def concepts(name, dag):
    """Check the relationships that explain each consolidated walkthrough."""
    jobs = dag["jobs"]
    if name == "selectors":
        for job in jobs:
            if job["operation"] == "calibrate":
                require(job["inputs"]["calibration"][0]["entities"]["revision"] == "2", "wrong revision")
                require(len(job["verify"]) == 1, "missing input verification")
            if job["operation"] == "compare":
                series, reference = job["inputs"]["series"][0], job["inputs"]["reference"][0]
                require(series["entities"]["station"] == reference["entities"]["station"], "wrong station")
            if job["operation"] == "split_bands":
                require(set(job["outputs"]) == {"low", "high"}, "bands must share one producer")
                require(job["command"] == [
                    ["band_split"], [{"path": job["inputs"]["series"][0]["path"]}],
                    ["--low"], [{"path": job["outputs"]["low"]["path"]}],
                    ["--high"], [{"path": job["outputs"]["high"]["path"]}],
                ], "band command must bind each output to its own flag")
            if job["operation"] == "summarise":
                days = [a["entities"]["day"] for a in job["inputs"]["days"]]
                require(days == (["1", "2", "10"] if len(days) == 3 else ["1", "2"]), "wrong collection order")
                require(len(job["inputs"]["policy"]) == 1, "policy must be single beside collection")
    elif name == "cohort":
        for job in jobs:
            for artifacts in job["inputs"].values():
                for artifact in artifacts:
                    e = artifact["entities"]
                    require(e.get("sub") != "03", "dropped subject reaches a job")
                    require(not (artifact["product"] == "bold" and e == {"sub": "02", "ses": "02", "run": "2"}), "excluded run reaches a job")
        require({a["path"] for a in dag["targets"]} == {
            "derivatives/sub-01/sub-01_long.nii.gz", "derivatives/sub-02/sub-02_long.nii.gz"
        }, "optional session/stage groups must disappear on subject outputs")
    elif name == "ragged_sweep":
        training = [j for j in jobs if j["operation"] == "train"]
        pairs = Counter((j["outputs"]["output"]["entities"]["config"],
                         j["outputs"]["output"]["entities"]["seed"]) for j in training)
        require(pairs == {("deep", "1"): 2, ("fast", "1"): 2, ("fast", "2"): 2}, "each must preserve observed seed pairs")
        board = next(j for j in jobs if j["operation"] == "leaderboard")
        require([(a["entities"]["model"], a["entities"]["config"]) for a in board["inputs"]["summaries"]] == [
            ("large", "deep"), ("large", "fast"), ("small", "deep"), ("small", "fast")
        ], "dimension order must determine summary order")
    elif name == "imported":
        require({tuple(j["stage"]) for j in jobs} == {
            ("preprocess", "clean"), ("preprocess", "combine"), ("preprocess",), ("analysis",)
        }, "missing nested or outer stage")
        require(all(a["path"].startswith("results/tally/") for a in dag["targets"]), "missing analysis path override")
        require(all(a["product"] == "text::shard" for a in dag["external_inputs"]), "source import lost its namespace")
    elif name in {"act", "somatic"}:
        require(all(j["origin"] is not None for j in jobs), "composite job lost its origin")
        if name == "somatic":
            for instance in ["normal_bam", "tumour_bam"]:
                marking = [j for j in jobs if any(a["product"] == instance for a in j["outputs"].values())]
                require(len(marking) == 1 and len(marking[0]["checks"]) == 2, "body and caller checks must reach BAM producer")
