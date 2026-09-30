#!/usr/bin/env python3
"""Build the guide a trial agent reads: the README's user-facing sections, then
the language reference, with `cargo run -- ` written as `spit `.

usage: build_guide.py <output file>
"""
import os
import re
import sys

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

readme = open(os.path.join(REPO, "README.md")).read()
reference = open(os.path.join(REPO, "docs", "language-reference.md")).read()

intro = re.sub(r"<img[^>]*>\n\n", "", readme[: readme.index("## Contents")])
body = readme[readme.index("## The three steps") : readme.index("## Language reference")]
how = readme[readme.index("## How SPIT works") : readme.index("## Documentation")]
reference = reference[reference.index("\n", reference.index("This is the full syntax")) + 1 :]

guide = intro + body + how + "\n# Language reference\n" + reference
open(sys.argv[1], "w").write(guide.replace("cargo run -- ", "spit "))
