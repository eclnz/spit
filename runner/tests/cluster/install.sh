#!/bin/bash
# Put this checkout's SPIT, spit-bash and runnable examples on the running test cluster's shared folder.
# Builds SPIT here, downloads the runner's wheels here for the nodes' Python, and installs them on the
# login node without a network, so the nodes need no route to crates.io or PyPI. Run from anywhere.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
compose=(docker compose -f "$here/compose.yaml")
stage=$(mktemp -d)
trap 'rm -rf "$stage" "$stage-spit"' EXIT

arch=$("${compose[@]}" exec -T login uname -m)
if [ "$(uname -s)" = Linux ] && [ "$(uname -m)" = "$arch" ]; then
    cargo build --release -q --manifest-path "$repo/Cargo.toml"
    spit_bin=$repo/target/release/spit
else
    # A host that is not the nodes' Linux builds SPIT in a container, into its own target folder.
    docker run --rm -v "$repo":/src -v spit-cluster-target:/target -v spit-cluster-cargo:/usr/local/cargo/registry \
        -e CARGO_TARGET_DIR=/target -w /src rust:latest cargo build --release -q
    cid=$(docker create -v spit-cluster-target:/target alpine true)
    docker cp "$cid:/target/release/spit" "$stage-spit" && docker rm "$cid" >/dev/null
    spit_bin=$stage-spit
fi
python=$("${compose[@]}" exec -T login python3 -c 'import sys; print(f"{sys.version_info[0]}{sys.version_info[1]}")')
python3 -m pip download -q --dest "$stage/wheels" --only-binary=:all: --python-version "$python" \
    --platform "manylinux2014_$arch" --platform "manylinux_2_17_$arch" --platform "manylinux_2_28_$arch" \
    'pydantic>=2,<3' 'xxhash>=3,<4' 'setuptools>=68' wheel

mkdir -p "$stage/spit/bin"
cp "$spit_bin" "$stage/spit/bin/spit"
cp -r "$repo/runner" "$stage/spit/runner"
# A build folder left by an earlier install would be taken as current, as the copy gives every file a new time.
rm -rf "$stage/spit/runner/build" "$stage"/spit/runner/src/*.egg-info
cp -r "$repo/examples/runnable" "$stage/spit/examples"
mv "$stage/wheels" "$stage/spit/wheels"
find "$stage/spit" -name __pycache__ -prune -exec rm -rf {} +

# Into the storage node's export, which every other node sees as /shared.
"${compose[@]}" exec -T storage rm -rf /export/spit
tar -C "$stage" -c spit | "${compose[@]}" exec -T storage tar -C /export -x
"${compose[@]}" exec -T storage chown -R hpc:hpc /export/spit
# The venv is made on the storage node, which sees its own writes at once, under the path the
# other nodes see: a venv records its absolute path, and the others' caches would lag behind.
"${compose[@]}" exec -T storage bash -c 'rmdir /shared 2>/dev/null; ln -sfn /export /shared'
"${compose[@]}" exec -T -u hpc storage bash -c '
    python3 -m venv /shared/spit/venv
    /shared/spit/venv/bin/pip install -q --no-index --find-links /shared/spit/wheels setuptools wheel
    /shared/spit/venv/bin/pip install -q --no-index --find-links /shared/spit/wheels --no-build-isolation /shared/spit/runner
    /shared/spit/venv/bin/spit-bash --version'
echo "installed: on a node, PATH=/shared/spit/bin:/shared/spit/venv/bin:\$PATH"
