#!/usr/bin/env python3
"""Build a trial guide from the site's learning pages and language manual.

usage: build_guide.py <output file>
"""
import os
import posixpath
import re
import sys
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
readme = (repo / "README.md").read_text()
intro = re.sub(r"<img[^>]*>\n\n", "", readme.split("## Try it", 1)[0])
chapters = [
    "docs/guide/concepts.md",
    "docs/guide/pipelines.md",
    "docs/guide/types-and-reuse.md",
    "docs/guide/matching.md",
    "docs/guide/paths.md",
    "docs/guide/recipes.md",
    "docs/guide/inspection.md",
    "docs/guide/runnable.md",
    "docs/language-reference.md",
]


def trial_link(match, chapter):
    label, target = match.groups()
    if target.startswith(("https://", "http://", "mailto:")):
        return match.group(0)
    if target.startswith("#"):
        target = chapter + target
    else:
        target = posixpath.normpath(posixpath.join(posixpath.dirname(chapter), target))
    return f"][{label}]({target})".replace("][", "[")


parts = [intro]
for chapter in chapters:
    content = (repo / chapter).read_text()
    content = re.sub(
        r"\[([^]]+)\]\(([^)]+)\)",
        lambda match: trial_link(match, chapter),
        content,
    )
    parts.append(content)

Path(sys.argv[1]).write_text("\n".join(parts).replace("cargo run -- ", "spit "))
