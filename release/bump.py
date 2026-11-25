#!/usr/bin/env python3
import argparse
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
VERSION = re.compile(r"^0\.(\d+)\.(\d+)$")


def current() -> str:
    found = re.search(r'^version = "([^"]+)"', (REPO / "Cargo.toml").read_text(), re.M)
    if found is None:
        sys.exit("Cargo.toml has no version line")
    return found.group(1)


def next_version(now: str, step: str) -> str:
    parts = VERSION.match(now)
    if parts is None:
        sys.exit(f"the current version {now} is not 0.MINOR.PATCH")
    minor, patch = int(parts.group(1)), int(parts.group(2))
    if step == "patch":
        return f"0.{minor}.{patch + 1}"
    if step == "minor":
        return f"0.{minor + 1}.0"
    if VERSION.match(step) is None:
        sys.exit(f"give patch, minor or a version 0.MINOR.PATCH, not {step}")
    return step


def replace_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text()
    changed, count = re.subn(pattern, replacement, text, count=1, flags=re.M)
    if count != 1:
        sys.exit(f"found no version to change in {path}")
    path.write_text(changed)


def bump_spit(version: str) -> list[Path]:
    files = [REPO / "Cargo.toml", REPO / "Cargo.lock", REPO / "runner/pyproject.toml"]
    replace_once(files[0], r'^version = "[^"]+"', f'version = "{version}"')
    replace_once(files[1], r'(^name = "spit"\nversion = )"[^"]+"', rf'\g<1>"{version}"')
    replace_once(files[2], r'^version = "[^"]+"', f'version = "{version}"')
    return files


def bump_vscode(folder: Path, version: str) -> list[Path]:
    package = folder / "package.json"
    lock = folder / "package-lock.json"
    for path in (package, lock):
        if not path.is_file():
            sys.exit(f"{path} does not exist; name spit-vscode's checkout with --vscode")
    replace_once(package, r'^(  "version": )"[^"]+"', rf'\g<1>"{version}"')
    replace_once(lock, r'^(  "version": )"[^"]+"', rf'\g<1>"{version}"')
    replace_once(lock, r'^(  "packages": \{\n    "": \{\n(?:      .*\n)*?      "version": )"[^"]+"', rf'\g<1>"{version}"')
    for path in (package, lock):
        json.loads(path.read_text())
    return [package, lock]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Set the version SPIT, its runner and spit-vscode share, in every file that holds it."
    )
    parser.add_argument("step", help="patch, minor, or a version 0.MINOR.PATCH")
    parser.add_argument("--vscode", type=Path, metavar="DIR", help="spit-vscode's checkout, to bump it as well")
    args = parser.parse_args()
    now = current()
    version = next_version(now, args.step)
    changed = bump_spit(version)
    if args.vscode is not None:
        changed += bump_vscode(args.vscode, version)
    print(f"{now} -> {version}")
    for path in changed:
        print(f"  {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
