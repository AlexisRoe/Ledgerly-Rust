#!/usr/bin/env bash
# `just flush`: empties the data of the Compose stack, so that the next test suite starts from a clean slate.
# Postgres and the object store are removed with their volumes; Postgres then gets a freshly migrated, empty
# schema (Flyway, as `just migrate`). A service that ran before runs again afterwards; one that did not exist or
# was stopped is left that way. ClamAV (minutes of signature download) and Keycloak (the vendored realm, which
# tests do not change) are kept. The `local` app fills an empty database with the demo data at its next start.
set -euo pipefail

compose=(docker compose --profile app)
services=(postgres seaweedfs)

was_running=()
for service in "${services[@]}"; do
  if [ -n "$("${compose[@]}" ps --quiet --status running "$service" 2>/dev/null)" ]; then
    was_running+=("$service")
  fi
done

echo "flush: removing the data of ${services[*]}" >&2
"${compose[@]}" rm --stop --force --volumes "${services[@]}" >/dev/null 2>&1 || true

if [ "${#was_running[@]}" -gt 0 ]; then
  "${compose[@]}" up --detach "${was_running[@]}" >/dev/null 2>&1
  if [[ " ${was_running[*]} " == *" postgres "* ]]; then
    "${compose[@]}" run --rm flyway >/dev/null
  fi
  echo "flush: ${was_running[*]} running again, with no data" >&2
fi
