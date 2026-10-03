#!/bin/bash
# Resolve every answer key again with the current binary, and fail if any key's
# jobs change. Run it after a change to the resolver: a key that no longer
# resolves, or resolves to other jobs, means the scenario needs a look before
# the next round of trials.
#
# usage: rebuild_keys.sh [--write]    --write replaces the stored keys
#
# The scenarios are kept in scenarios.zip; each run unpacks them to a temporary
# folder, and --write packs them back with the new keys.
set -e
HARNESS=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HARNESS/../.." && pwd)
BIN=${SPIT_BIN:-$REPO/target/release/spit}
WRITE=$1
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
status=0
python3 "$HARNESS/archive.py" unpack "$HARNESS/scenarios.zip" "$TMP"

check() { # name, expected, fresh
  if [ "$WRITE" = --write ]; then
    # A key's root is wherever it was resolved; the grader ignores it.
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); d["root"]=None; open(sys.argv[2],"w").write(json.dumps(d))' "$3" "$2"
    echo "wrote $1"; return
  fi
  if python3 "$HARNESS/grade.py" "$2" "$3" | grep -q '^PASS'; then echo "ok    $1"
  else echo "FAIL  $1"; python3 "$HARNESS/grade.py" "$2" "$3"; status=1; fi
}

resolve() { # name, recipe, spitdag: resolve the key, or say why it cannot
  if "$BIN" dag "$2" -o "$3" 2>"$TMP/error"; then return 0; fi
  echo "FAIL  $1: dag failed"; sed 's/^/      /' "$TMP/error"; status=1; return 1
}

for S in "$TMP"/scenarios/*/; do
  name=$(basename "$S"); W=$TMP/$name
  cp -r "$S/data" "$W"; cp "$S"/key/*.spit* "$W"/ 2>/dev/null || true
  rm -f "$W/expected.spitdag"
  resolve "$name" "$W/dataset.spitin" "$TMP/$name.spitdag" &&
    check "$name" "$S/key/expected.spitdag" "$TMP/$name.spitdag"

  if [ -d "$S/key-followup" ]; then
    W=$TMP/$name-followup
    cp -r "$S/data" "$W"; cp -r "$S/addition/." "$W/"
    cp "$S/key-followup/pipeline.spit" "$S/key-followup/dataset.spitin" "$W/"
    resolve "$name follow-up" "$W/dataset.spitin" "$TMP/$name-followup.spitdag" &&
      check "$name follow-up" "$S/key-followup/expected.spitdag" "$TMP/$name-followup.spitdag"
  fi
done
if [ "$WRITE" = --write ]; then
  python3 "$HARNESS/archive.py" pack "$TMP/scenarios" "$HARNESS/scenarios.zip"
fi
exit $status
