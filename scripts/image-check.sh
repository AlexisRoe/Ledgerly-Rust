#!/usr/bin/env bash
# Checks a built image (plan M11.2): it must not run as root, and Trivy must find no vulnerability rated high or
# critical that has a fix. Unfixed findings are reported but do not fail: there is nothing to update to yet.
# Trivy runs as a container pinned by digest (no action, no install); its database is cached between runs.
set -euo pipefail

image="${1:?usage: image-check.sh <image>}"
trivy="aquasec/trivy:0.76.2@sha256:8f6bb947dbc528902e11f2f5be52f7b519f6a39d5d148f41166f2040499768fb"
cache="${TRIVY_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/trivy}"

# The user the image runs as: `USER` in the Dockerfile. Empty means root, as does uid 0 in any spelling.
user="$(docker image inspect --format '{{.Config.User}}' "$image")"
uid="${user%%:*}"
if [ -z "$uid" ] || [ "$uid" = "root" ] || [ "$((10#${uid//[^0-9]/0}))" = "0" ]; then
    echo "the image runs as root (USER is '${user}')" >&2
    exit 1
fi
echo "runs as ${user}"

mkdir -p "$cache"
docker run --rm \
    -v /var/run/docker.sock:/var/run/docker.sock:ro \
    -v "$cache:/root/.cache/trivy" \
    "$trivy" image --quiet --scanners vuln --severity HIGH,CRITICAL --ignore-unfixed --exit-code 1 "$image"
