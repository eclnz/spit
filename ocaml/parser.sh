#!/bin/sh
set -eu
parser_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
parser_build=$(mktemp -d "${TMPDIR:-/tmp}/spit-parser.XXXXXX")
trap 'rm -rf "$parser_build"' EXIT HUP INT TERM
if [ "${1:-}" = --test ]; then
    "$parser_directory/build.sh" parser_tests "$parser_build"
    "$parser_build/parser_tests" "$parser_directory/examples"
else
    "$parser_directory/build.sh" parse "$parser_build"
    if [ "$#" -eq 0 ]; then
        cd "$parser_directory/examples"
    fi
    "$parser_build/parse" "$@"
fi
