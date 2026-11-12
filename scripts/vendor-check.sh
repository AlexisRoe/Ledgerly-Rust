#!/usr/bin/env bash
# Vendor drift check (ADR 0007, plan M0.2).
#
# Fails if any file vendored from Ledgerly-Java differs from vendor/ledgerly-java.SHA256SUMS, is missing, or if a
# file the manifest does not list exists under vendor/ledgerly-java/, api/ or bruno/. Works offline.
#
# vendor/ledgerly-java/SOURCE is the one file allowed without a checksum: it is not copied from upstream, it
# records which upstream commit the copies come from.
# Every offending file is named on stderr.
set -euo pipefail

cd "$(dirname "$0")/.."

manifest="vendor/ledgerly-java.SHA256SUMS"
roots=(vendor/ledgerly-java api bruno)
provenance="vendor/ledgerly-java/SOURCE"

if command -v shasum >/dev/null 2>&1; then
    sha256=(shasum -a 256)
else
    sha256=(sha256sum)
fi

status=0

# 1. Every listed file exists and has the recorded checksum. Paths may contain spaces; `-c` handles that.
if ! output="$("${sha256[@]}" -c "$manifest" 2>&1)"; then
    echo "vendor-check: vendored files differ from $manifest:" >&2
    grep -v ': OK$' <<<"$output" | sed 's/^/  /' >&2
    status=1
fi

# 2. Nothing unlisted sits next to the vendored files. A manifest line is "<64 hex>  <path>".
listed="$(sed -E 's/^[0-9a-f]{64}  //' "$manifest" | LC_ALL=C sort)"
present="$(find "${roots[@]}" ! -type d ! -path "$provenance" | LC_ALL=C sort)"
unlisted="$(LC_ALL=C comm -13 <(printf '%s\n' "$listed") <(printf '%s\n' "$present"))"
if [[ -n "$unlisted" ]]; then
    echo "vendor-check: files not listed in $manifest:" >&2
    sed 's/^/  /' <<<"$unlisted" >&2
    status=1
fi

if [[ ! -f "$provenance" ]]; then
    echo "vendor-check: $provenance is missing (it names the pinned Ledgerly-Java commit)" >&2
    status=1
fi

if [[ "$status" -eq 0 ]]; then
    echo "vendor-check: $(wc -l <"$manifest" | tr -d ' ') vendored files match $manifest"
fi
exit "$status"
