#!/usr/bin/env python3
"""Time the `spit` CLI on generated pipelines and datasets, and profile it.

usage:
    bench.py pipeline [--old BIN] [--new BIN] [--steps 1000,2000,4000]
    bench.py dataset  [--old BIN] [--new BIN] [--subjects 1000,4000] [--extra 0,60]
    bench.py profile  [--new BIN] [--steps 1000] [-- spit-args ...]

`pipeline` times `spit check` on a chain of steps, the size that once made
the path checks quadratic: plain, with `--json`, and with `--json --hovers`.
`dataset` times `spit inputs`, `spit dag` and `spit check` on a BIDS-like
dataset of subjects, sessions and runs, with `extra` more steps after the
first four, and once more with `ext:` completing the default path.
`profile` runs one `spit check` on a chain under callgrind and prints the
functions that take the most instructions, inclusive.

`--new` defaults to this repository's `target/release/spit`; build it with
`cargo build --release` first. `--old` is another build to compare with,
such as one from a worktree of an earlier commit. A build from before
`{@product}` is given the old `{product}` spelling automatically.

Each time is the quickest of several runs, in milliseconds. Generated files
go in `profiling/work/`, which git ignores; datasets there are reused.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, "work")
DEFAULT_NEW = os.path.join(HERE, "..", "target", "release", "spit")


def uses_at(binary):
    """Whether `binary` spells built-in placeholders `{@product}`."""
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "probe.spit")
        with open(path, "w") as file:
            file.write("path: out/{@product}/{@entities}\nsource raw [id]\npath raw: in/{id}\n")
        return subprocess.run([binary, "check", path], capture_output=True).returncode == 0


def placeholders(binary):
    at = "@" if uses_at(binary) else ""
    return "{%sproduct}" % at, "{%sentities}" % at


def chain(binary, steps):
    """A pipeline of `steps` steps, each reading the one before."""
    product, entities = placeholders(binary)
    lines = [
        f"path: out/{product}/{entities}.txt",
        "source raw : T [sub]",
        "path raw: in/{sub}.txt",
        "operation step(x: T) -> T",
    ]
    previous = "raw"
    for index in range(steps):
        lines.append(f"p{index} = step({previous})")
        previous = f"p{index}"
    return "\n".join(lines) + "\n"


def study(binary, extra, ext):
    """The scaling test's pipeline, with `extra` steps after its four, and
    with `ext:` completing the default path when `ext` is set."""
    product, entities = placeholders(binary)
    if ext:
        lines = [f"path: out/{product}/{entities}", "ext: .txt"]
    else:
        lines = [f"path: out/{product}/{entities}.txt"]
    lines += [
        "source image : Image [sub, ses, run]",
        "path image: sub-{sub}/ses-{ses}/image_run-{run}.nii",
        "source mask : Mask [sub]",
        "path mask: sub-{sub}/mask.nii",
        "source reference : Reference [sub, ses]",
        "path reference: sub-{sub}/ses-{ses}/reference.nii",
        "operation clean(image: Image, mask: Mask) -> Image" + (" .nii.gz" if ext else ""),
        "operation align(image: Image, reference: Reference) -> Image",
        "operation average(images: many Image) -> Image",
        "operation compare(image: Image, reference: Reference) -> Score",
        "operation touch(image: Image) -> Image",
        "cleaned = clean(image, mask)",
        "aligned = align(cleaned, reference)",
        "averaged = average(aligned @ vary(run))",
        "score = compare(averaged, reference)",
    ]
    previous = "averaged"
    for index in range(extra):
        lines.append(f"t{index} = touch({previous})")
        previous = f"t{index}"
    return "\n".join(lines) + "\n"


RECIPE = """\
pipeline analysis.spit
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
require image count>=2 per [sub, ses]
require reference count=1 per [sub, ses]
drop [sub, ses] where image count<1
"""


def dataset(subjects):
    """A dataset of `subjects` subjects, each with a mask and two sessions of
    a reference and two runs: seven files a subject."""
    root = os.path.join(WORK, f"data-{subjects}")
    if os.path.isdir(root):
        return root
    partial = root + ".partial"
    shutil.rmtree(partial, ignore_errors=True)
    for sub in range(1, subjects + 1):
        subject = os.path.join(partial, f"sub-{sub:04}")
        os.makedirs(subject)
        open(os.path.join(subject, "mask.nii"), "w").close()
        for ses in (1, 2):
            session = os.path.join(subject, f"ses-{ses}")
            os.makedirs(session)
            open(os.path.join(session, "reference.nii"), "w").close()
            for run in (1, 2):
                open(os.path.join(session, f"image_run-{run}.nii"), "w").close()
    os.rename(partial, root)
    return root


def write(folder, name, text):
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, name), "w") as file:
        file.write(text)


def quickest(command, folder, repeats):
    """The quickest of `repeats` runs of `command` in `folder`, in ms."""
    times = []
    for _ in range(repeats):
        start = time.perf_counter()
        result = subprocess.run(command, cwd=folder, capture_output=True, text=True)
        times.append(time.perf_counter() - start)
        if result.returncode != 0:
            sys.exit(f"failed: {' '.join(command)} in {folder}\n{result.stderr[:2000]}")
    return min(times) * 1000


def builds(args):
    named = [("new", os.path.abspath(args.new))]
    if args.old:
        named.insert(0, ("old", os.path.abspath(args.old)))
    for _, binary in named:
        if not os.access(binary, os.X_OK):
            sys.exit(f"no spit build at {binary}; run `cargo build --release`")
    return named


def numbers(text):
    return [int(value) for value in text.split(",")]


def run_pipeline(args):
    print(f"{'steps':>6}  {'build':<5} {'check':>10} {'--json':>10} {'--hovers':>10}")
    for steps in numbers(args.steps):
        for name, binary in builds(args):
            folder = os.path.join(WORK, f"chain-{name}-{steps}")
            write(folder, "chain.spit", chain(binary, steps))
            times = [quickest([binary, "check", "chain.spit"], folder, args.repeats)]
            times.append(quickest([binary, "check", "chain.spit", "--json"], folder, args.repeats))
            if uses_at(binary):
                hovers = [binary, "check", "chain.spit", "--json", "--hovers"]
                times.append(quickest(hovers, folder, args.repeats))
            cells = "".join(f"{value:>10.1f}" for value in times)
            print(f"{steps:>6}  {name:<5}{cells}")


def run_dataset(args):
    print(f"{'subjects':>8} {'extra':>5}  {'build':<9} {'inputs':>9} {'dag':>9} {'check':>9}")
    for subjects in numbers(args.subjects):
        root = dataset(subjects)
        for extra in numbers(args.extra):
            for name, binary in builds(args):
                variants = [(name, False)]
                if name == "new" and uses_at(binary):
                    variants.append(("new+ext", True))
                for label, ext in variants:
                    folder = os.path.join(WORK, f"study-{label}-{subjects}-{extra}")
                    write(folder, "analysis.spit", study(binary, extra, ext))
                    # The recipe names the dataset, and the .spitout records it.
                    recipe = RECIPE.replace("\n", f"\nroot {root}\n", 1)
                    write(folder, "dataset.spitin", recipe)
                    inputs = [binary, "inputs", "dataset.spitin", "-o", "d.spitout"]
                    dag = [binary, "dag", "analysis.spit", "d.spitout", "-o", "a.spitdag"]
                    times = [
                        quickest(inputs, folder, args.repeats),
                        quickest(dag, folder, args.repeats),
                        quickest([binary, "check", "analysis.spit"], folder, args.repeats),
                    ]
                    cells = "".join(f"{value:>9.1f}" for value in times)
                    print(f"{subjects:>8} {extra:>5}  {label:<9}{cells}")


def run_profile(args):
    if not shutil.which("valgrind"):
        sys.exit("profile needs valgrind, for callgrind")
    binary = os.path.abspath(args.new)
    steps = numbers(args.steps)[0]
    folder = os.path.join(WORK, f"profile-{steps}")
    write(folder, "chain.spit", chain(binary, steps))
    output = os.path.join(folder, "callgrind.out")
    command = [binary, "check", "chain.spit", *args.spit]
    subprocess.run(
        ["valgrind", "--tool=callgrind", f"--callgrind-out-file={output}", *command],
        cwd=folder,
        capture_output=True,
    )
    report = subprocess.run(
        ["callgrind_annotate", "--inclusive=yes", output], capture_output=True, text=True
    ).stdout
    shown = 0
    for line in report.splitlines():
        if "spit::" in line or "PROGRAM TOTALS" in line:
            print(line[:180])
            shown += 1
            if shown >= args.top:
                break
    print(f"\nfull profile: {output}")
    print(f"open it with: callgrind_annotate --inclusive=yes {output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mode", choices=["pipeline", "dataset", "profile"])
    parser.add_argument("--new", default=DEFAULT_NEW, help="the build to time (default: target/release/spit)")
    parser.add_argument("--old", help="another build to compare with")
    parser.add_argument("--steps", default="1000,2000,4000", help="chain lengths for pipeline and profile")
    parser.add_argument("--subjects", default="1000,4000", help="dataset sizes for dataset")
    parser.add_argument("--extra", default="0,60", help="steps added to the dataset's pipeline")
    parser.add_argument("--repeats", type=int, default=5, help="runs per time; the quickest is kept")
    parser.add_argument("--top", type=int, default=30, help="functions profile prints")
    parser.add_argument("spit", nargs="*", help="after --, extra arguments to `spit check` for profile")
    args = parser.parse_args()
    {"pipeline": run_pipeline, "dataset": run_dataset, "profile": run_profile}[args.mode](args)


if __name__ == "__main__":
    main()
