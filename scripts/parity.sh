#!/usr/bin/env bash
# `just parity [url]`: runs the parity suite (crates/parity-tests/src/bin/parity.rs) and, however it ends
# (success, failure or Ctrl-C), removes every container it created, with its anonymous volumes. Containers
# that existed before the run, such as a stopped or running dev environment, are left as they were: running if
# they ran, stopped if they were stopped.
set -euo pipefail

url="${1:-}"
compose=(docker compose --profile app)

containers() { "${compose[@]}" ps --all --quiet | sort; }
running() { "${compose[@]}" ps --quiet | sort; }

before="$(containers)"
running_before="$(running)"

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  # The app containers the runner starts per run group (`docker compose run --name ledgerly-parity-<group>`).
  docker ps --all --quiet --filter "name=^ledgerly-parity-" | xargs -r docker rm --force --volumes >/dev/null || true
  # Whatever `compose run app` brought up besides: Postgres, the object store, ClamAV, the finished Flyway.
  local created
  created="$(comm -13 <(echo "$before") <(containers))" || true
  if [ -n "$created" ]; then
    echo "parity: removing the containers this run started" >&2
    echo "$created" | xargs docker rm --force --volumes >/dev/null || true
  fi
  # Existing containers the run had to start (`compose run app` starts its dependencies) are stopped again.
  local started
  started="$(comm -12 <(echo "$before") <(comm -13 <(echo "$running_before") <(running)))" || true
  if [ -n "$started" ]; then
    echo "parity: stopping the containers this run started" >&2
    echo "$started" | xargs docker stop >/dev/null || true
  fi
  # With nothing of the project left, also the network compose created.
  if [ -z "$(containers)" ]; then
    "${compose[@]}" down --remove-orphans >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

if [ -z "$url" ]; then
  just image
fi
cargo run --locked --quiet -p ledgerly-parity-tests --bin parity -- ${url:+"$url"}
