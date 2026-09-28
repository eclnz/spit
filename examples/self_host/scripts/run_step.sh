#!/usr/bin/env bash
# Runs a command and records its combined output as the artifact SPIT expects
# at $1. SPIT command templates cannot redirect output themselves ("Command
# templates give ordered words and arguments, not shell pipelines or
# redirection"), so a job whose real tool has no output-path argument -
# such as `cargo build` - is wrapped by a small script that takes the
# output path as its first argument instead.
set -u
output="$1"
shift
mkdir -p "$(dirname "$output")"
if "$@" >"$output" 2>&1; then
    exit 0
fi
status=$?
echo "FAILED (exit $status): $*" >>"$output"
exit "$status"
