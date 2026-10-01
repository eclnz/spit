#!/usr/bin/env python3
"""Check local links in a built trial's guide and bundled documentation."""

import re
import sys
from pathlib import Path
from urllib.parse import unquote


root = Path(sys.argv[1]).resolve()
documents = [root / "GUIDE.md", root / "README.md", *sorted((root / "docs").glob("*.md"))]
errors = []


def anchors(document):
    result = set()
    counts = {}
    for heading in re.findall(r"^#{1,6}\s+(.+)$", document.read_text(), re.M):
        heading = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", heading)
        heading = re.sub(r"<[^>]+>|[`*_~]", "", heading).lower()
        slug = re.sub(r"[^\w\- ]", "", heading).replace(" ", "-")
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(f"{slug}-{count}" if count else slug)
    return result


for document in documents:
    content = document.read_text()
    for link in re.findall(r"!?(?:\[[^]]*\])\(([^)]+)\)", content):
        if "://" in link or link.startswith("mailto:"):
            continue
        path_part, _, anchor = unquote(link).partition("#")
        target = (document.parent / path_part).resolve() if path_part else document
        if not target.is_relative_to(root) or not target.exists():
            errors.append(f"{document.relative_to(root)}: missing {link}")
        elif anchor and target.suffix == ".md" and anchor not in anchors(target):
            errors.append(f"{document.relative_to(root)}: missing anchor {link}")

for forbidden in [root / "ANSWER.md", *root.rglob("*.spitdag")]:
    if forbidden.exists():
        errors.append(f"trial contains an answer or plan: {forbidden.relative_to(root)}")

if errors:
    raise SystemExit("\n".join(errors))
print("guide links resolve")
