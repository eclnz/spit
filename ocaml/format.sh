#!/bin/sh
set -eu
format_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
format_mode=--inplace
if [ "${1:-}" = --check ]; then format_mode=--check; fi
find "$format_root/src" "$format_root/bin" "$format_root/tests" -name '*.ml' \
    -exec "${SPIT_OCAMLFORMAT:-ocamlformat}" "$format_mode" {} +
