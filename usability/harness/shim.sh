#!/bin/bash
# Logging wrapper around the real spit binary. Each call gets a numbered folder
# under the run's log directory with its arguments, output, exit code, and a
# snapshot of every .spit/.spitin/.spitout file in the sandbox at that moment.
REAL="__REAL__"
LOG="__LOG__"
SANDBOX="__SANDBOX__"

mkdir -p "$LOG"
n=$(( $(ls -1 "$LOG" 2>/dev/null | grep -c '^[0-9]') + 1 ))
d="$LOG/$(printf '%03d' "$n")"
mkdir -p "$d/files"
{
  printf 'time: %s\ncwd: %s\nargs:' "$(date -u +%FT%T)" "$PWD"
  printf ' %q' "$@"
  printf '\n'
} > "$d/call.txt"
(cd "$SANDBOX" && find . \( -name '*.spit' -o -name '*.spitin' -o -name '*.spitout' \) -size -512k -print0 \
  | xargs -0 -r cp --parents -t "$d/files" 2>/dev/null)

if [ ! -t 0 ] && [[ " $* " == *" --stdin "* || " $* " == *" - "* ]]; then
  tee "$d/stdin" | "$REAL" "$@" > >(tee "$d/stdout") 2> >(tee "$d/stderr" >&2)
  rc=${PIPESTATUS[1]}
else
  "$REAL" "$@" > >(tee "$d/stdout") 2> >(tee "$d/stderr" >&2)
  rc=$?
fi
wait
echo "$rc" > "$d/exit"
exit "$rc"
