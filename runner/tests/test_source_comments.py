import ast
import io
import tokenize
import unittest
from collections.abc import Iterator
from pathlib import Path


def source_notes(source: bytes) -> Iterator[tuple[int, str]]:
    for token in tokenize.tokenize(io.BytesIO(source).readline):
        if token.type == tokenize.COMMENT:
            if token.start == (1, 0) and token.string.startswith("#!/"):
                continue
            yield token.start[0], "Python comments are forbidden"

    for node in ast.walk(ast.parse(source)):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
            if ast.get_docstring(node, clean=False) is not None:
                yield node.body[0].lineno, "Python docstrings are forbidden"


class SourceCommentsTest(unittest.TestCase):
    def test_python_sources_have_no_comments(self):
        root = Path(__file__).resolve().parents[1]
        violations = []
        for directory in ("src", "tests"):
            for file in sorted((root / directory).rglob("*.py")):
                for line, message in sorted(source_notes(file.read_bytes())):
                    violations.append(f"{file.relative_to(root)}:{line}: {message}")
        if violations:
            self.fail("\n" + "\n".join(violations))

    def test_distinguishes_comments_and_docstrings_from_strings(self):
        source = b'''#!/usr/bin/env python3
value = "# text"  # Inline
def example():
    """Documentation."""
    return """# More text."""
'''
        self.assertEqual(
            list(source_notes(source)),
            [(2, "Python comments are forbidden"), (4, "Python docstrings are forbidden")],
        )
