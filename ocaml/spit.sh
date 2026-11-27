#!/bin/sh
set -eu
compiler_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
compiler_build=$(mktemp -d "${TMPDIR:-/tmp}/spit-compiler.XXXXXX")
trap 'rm -rf "$compiler_build"' EXIT HUP INT TERM
"$compiler_directory/build.sh" spit "$compiler_build"
if [ "${1:-}" = --test ]; then
    python3 "$compiler_directory/tests/compiler.py" "$compiler_build/spit" "$(dirname -- "$compiler_directory")"
else
    "$compiler_build/spit" "$@"
fi
