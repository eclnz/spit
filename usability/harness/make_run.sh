#!/bin/bash
# Build one trial sandbox: the scenario's data, its brief, the guide, a report
# template, and bin/spit, a wrapper that logs every call.
#
# usage: make_run.sh <scenario> <tag>        e.g. make_run.sh s1-logs a
#
# SPIT_TRIALS    where sandboxes go (default /tmp/spit-trials)
# SPIT_PRIVATE   where the real binary and call logs go, outside every sandbox
#                (default /tmp/spit-trials-private)
# SPIT_BIN       the binary to test (default target/release/spit in this repo)
#
# Keep both folders away from this repository: an agent that reads its wrapper
# learns where they are, and must find nothing there but a binary and logs.
set -e
HARNESS=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HARNESS/../.." && pwd)
TRIALS=${SPIT_TRIALS:-/tmp/spit-trials}
PRIVATE=${SPIT_PRIVATE:-/tmp/spit-trials-private}
BIN=${SPIT_BIN:-$REPO/target/release/spit}

scen=$1; tag=$2
[ -n "$scen" ] && [ -n "$tag" ] || { echo "usage: make_run.sh <scenario> <tag>" >&2; exit 2; }
S=$HARNESS/scenarios/$scen
[ -d "$S" ] || { echo "no scenario $scen" >&2; exit 2; }
[ -x "$BIN" ] || { echo "no binary at $BIN; run cargo build --release" >&2; exit 2; }

R=$TRIALS/$scen-$tag
L=$PRIVATE/logs/$scen-$tag
REAL=$PRIVATE/spit-real
mkdir -p "$PRIVATE" && cp "$BIN" "$REAL"
rm -rf "$R" "$L"; mkdir -p "$R/bin" "$L"
cp -r "$S/data" "$R/data"
cp "$HARNESS/REPORT.md" "$R/"
python3 "$HARNESS/build_guide.py" "$R/GUIDE.md"
cp "$REPO/README.md" "$REPO/LICENSE" "$R/"
cp -r "$REPO/docs" "$REPO/examples" "$R/"
python3 "$HARNESS/check_guide_links.py" "$R"
cat "$S/brief.md" "$HARNESS/common.md" | sed "s#__SANDBOX__#$R#g" > "$R/TASK.md"
if [ "$scen" = s6-diagnose ]; then
  sed -i 's#^1\. `plan.spitdag`#1. `ANSWER.md` (Part 1) and `plan.spitdag` (Part 2)#' "$R/TASK.md"
  sed -i 's#^2\. Your pipeline file(s), left in the working folder\.#2. Any recipe or other files you wrote, left in the working folder.#' "$R/TASK.md"
fi
sed -e "s#__REAL__#$REAL#" -e "s#__LOG__#$L#" -e "s#__SANDBOX__#$R#" "$HARNESS/shim.sh" > "$R/bin/spit"
chmod +x "$R/bin/spit"
echo "$R"
