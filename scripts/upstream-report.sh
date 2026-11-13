#!/usr/bin/env bash
# Prints, as Markdown, how far Ledgerly-Java's main is ahead of the vendored commit, and which of the vendored
# files changed. Always exits 0: this is a report, not a check (ADR 0007). Needs the GitHub CLI.
set -uo pipefail

cd "$(dirname "$0")/.."
source_file=vendor/ledgerly-java/SOURCE
repository=$(sed -n 's|^repository: https://github.com/||p' "$source_file")
commit=$(sed -n 's|^commit: ||p' "$source_file")

echo "## Ledgerly-Java upstream"
echo
if [ -z "$repository" ] || [ -z "$commit" ]; then
    echo "Could not read the repository and commit from \`$source_file\`."
    exit 0
fi
if ! comparison=$(gh api "repos/$repository/compare/$commit...main" 2>/dev/null); then
    echo "Could not compare \`$commit\` with \`main\` of $repository (network, token or repository access)."
    exit 0
fi

ahead=$(jq -r '.ahead_by' <<<"$comparison")
if [ "$ahead" = "0" ]; then
    echo "Up to date: \`main\` of $repository is at the vendored commit \`${commit:0:7}\`."
    exit 0
fi

echo "\`main\` of $repository is **$ahead commit(s) ahead** of the vendored commit \`${commit:0:7}\`."
echo "[Compare](https://github.com/$repository/compare/$commit...main)"
echo
# The vendored files sit at their upstream paths, either at the root or under vendor/ledgerly-java/.
vendored=$(sed -E 's/^[0-9a-f]{64}  //; s|^vendor/ledgerly-java/||' vendor/ledgerly-java.SHA256SUMS)
changed=$(jq -r '.files[].filename' <<<"$comparison" | grep -Fx -f <(echo "$vendored") || true)
if [ -z "$changed" ]; then
    echo "None of the vendored files changed (of the first 300 changed files GitHub lists)."
else
    echo "Vendored files that changed upstream:"
    echo
    while IFS= read -r file; do echo "- \`$file\`"; done <<<"$changed"
fi
exit 0
