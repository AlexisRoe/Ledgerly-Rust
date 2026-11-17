#!/usr/bin/env bash
# For the course author: checks that the exercises compile as shipped and that every file in solutions/
# makes its tests pass. Learners don't need this.
set -euo pipefail
cd "$(dirname "$0")"
export CARGO_TARGET_DIR="$PWD/target/verify"

echo "== exercises as shipped must compile"
cargo build --quiet --all-targets

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp -R Cargo.toml src "$work/"
[ -f Cargo.lock ] && cp Cargo.lock "$work/"
for solution in solutions/*.rs; do
    name="$(basename "$solution")"
    if [ -f "src/bin/$name" ]; then cp "$solution" "$work/src/bin/$name"; else cp "$solution" "$work/src/$name"; fi
done
echo "== with solutions/, every test must pass"
(cd "$work" && cargo test --quiet 2>&1 | tail -n 15)
