#!/bin/sh
# Rebuild plan.spitdag. SPIT recipes cannot exclude a single artifact,
# so the scanned inventory is post-edited to drop runs listed in
# data/excluded_runs.tsv (columns: sub ses run) before resolving jobs.
set -e
W=/srv/spit-trials/s2-cohort-a
"$W/bin/spit" inputs "$W/data/cohort.spitin" -o "$W/data/cohort.scanned.spitout"
python3 - "$W/data/cohort.scanned.spitout" "$W/data/excluded_runs.tsv" "$W/data/cohort.spitout" <<'PY'
import re, sys
src, excl, dst = sys.argv[1:]
drop = set()
for line in open(excl):
    line = line.strip()
    if line and not line.startswith('#'):
        drop.add(tuple(line.split()))
lines = open(src).read().split('\n')
out, ctx, used = [], None, set()
for i, line in enumerate(lines):
    m = re.match(r'\s*\[sub=([^,\]]+),ses=([^,\]]+)\]:\s*$', line)
    if m:
        ctx = m.groups()
    m = re.match(r'(\s*)\[run=([^\]]+)\]:\s*$', line)
    if m and ctx and i + 1 < len(lines) and lines[i + 1].strip() == 'bold':
        runs = m.group(2).split(',')
        keep = [r for r in runs if (ctx[0], ctx[1], r) not in drop]
        used |= {(ctx[0], ctx[1], r) for r in runs} & drop
        if not keep:
            lines[i + 1] = ''
            continue
        line = f"{m.group(1)}[run={','.join(keep)}]:"
    out.append(line)
for d in sorted(drop - used):
    print(f"warning: excluded run {d} not found in the inventory", file=sys.stderr)
for d in sorted(used):
    print(f"note: excluded bold run sub={d[0]} ses={d[1]} run={d[2]}", file=sys.stderr)
open(dst, 'w').write('\n'.join(out))
PY
"$W/bin/spit" dag "$W/rest.spit" "$W/data/cohort.spitout" --root "$W/data" -o "$W/plan.spitdag"
