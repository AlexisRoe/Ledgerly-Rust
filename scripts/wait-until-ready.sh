#!/usr/bin/env bash
# Waits until a readiness probe answers 200, for at most two minutes. The app image has no shell and no curl, so
# Compose cannot run a health check inside it; this asks from outside.
set -euo pipefail

url="${1:?usage: wait-until-ready.sh <readiness-url>}"
for _ in $(seq 1 120); do
    if [ "$(curl --silent --output /dev/null --write-out '%{http_code}' --max-time 2 "$url" || true)" = "200" ]; then
        echo "ready: $url"
        exit 0
    fi
    sleep 1
done
echo "not ready after 120 s: $url" >&2
exit 1
