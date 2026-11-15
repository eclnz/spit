"""Check that the weather family uses every language feature the guide names.

`examples/runnable/weather/features.json` lists the features, each with a
pattern that a version must match, the files that may hold it, and the guide
section that explains it. The check fails when a feature is missing from its
files, when a SPIT keyword belongs to no feature, when a guide section does not
exist, or when the index page leaves a feature out.
"""

import json
import re
import subprocess
from pathlib import Path

from graph import require

FOLDER = "examples/runnable/weather"
INDEX = "docs/guide/weather.md"


def code(text):
    """`text` without the lines that are only a comment, so a comment cannot satisfy a pattern."""
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#")) + "\n"


def anchors(text):
    """The anchors of a Markdown page's headings, as the site spells them."""
    found, counts = set(), {}
    for heading in re.findall(r"^#{1,6}\s+(.+)$", text, re.M):
        heading = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", heading)
        heading = re.sub(r"<[^>]+>|[`*_~]", "", heading).lower()
        slug = re.sub(r"[^\w\- ]", "", heading).replace(" ", "-")
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        found.add(f"{slug}-{count}" if count else slug)
    return found


def features(repo, binary, manifest=None):
    """Raise an `AssertionError` that lists every way the family falls short of the manifest."""
    folder = repo / FOLDER
    manifest = manifest or json.loads((folder / "features.json").read_text())
    problems = []
    texts = {}

    def text(name):
        if name not in texts:
            path = folder / name
            require(path.is_file(), f"features.json names {name}, which is not in {FOLDER}")
            texts[name] = code(path.read_text())
        return texts[name]

    for version in manifest["versions"]:
        for suffix in (".spit", ".spitin"):
            require((folder / f"{version}{suffix}").is_file(), f"version {version} has no {suffix} file")

    pages = {}
    for feature in manifest["features"]:
        name = feature["name"]
        pattern = re.compile(feature["pattern"], re.M)
        if not any(pattern.search(text(file)) for file in feature["files"]):
            problems.append(f"{name}: no match for {feature['pattern']!r} in {', '.join(feature['files'])}")
        page, _, anchor = feature["guide"].partition("#")
        path = repo / "docs/guide" / page
        if page not in pages:
            pages[page] = anchors(path.read_text()) if path.is_file() else None
        if pages[page] is None or (anchor and anchor not in pages[page]):
            problems.append(f"{name}: the guide has no section {feature['guide']}")

    keywords = json.loads(subprocess.run([binary, "keywords"], capture_output=True, text=True, check=True).stdout)
    known = {entry["word"] for entry in keywords["keywords"]}
    used = {feature["keyword"] for feature in manifest["features"] if "keyword" in feature}
    for word in sorted(known - used - set(manifest["retired"])):
        problems.append(f"keyword `{word}` belongs to no feature")
    for word in sorted((used | set(manifest["retired"])) - known):
        problems.append(f"`{word}` is not a SPIT keyword")

    index = repo / INDEX
    if index.is_file():
        listed = index.read_text()
        for feature in manifest["features"]:
            if feature["name"] not in listed:
                problems.append(f"{INDEX} does not list: {feature['name']}")
    else:
        problems.append(f"{INDEX} does not exist")

    require(not problems, "the weather family falls short of its features:\n  " + "\n  ".join(problems))
