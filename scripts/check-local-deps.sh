#!/usr/bin/env bash
set -euo pipefail

pubsub_host="${PUBSUB_EMULATOR_HOST:-127.0.0.1:8085}"
clamd_address="${CLAMD_ADDRESS:-127.0.0.1:3310}"
pgbouncer_address="${PGBOUNCER_ADDRESS:-127.0.0.1:6432}"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker compose ps >/dev/null 2>&1; then
  echo "docker compose is not available from this directory" >&2
  exit 1
fi

docker compose exec -T postgres pg_isready -U proofplane -d proofplane >/dev/null

# Checked from the host, not with `docker compose exec`: runtime traffic reaches
# the pooler over the published port, so that is the path worth proving.
pgbouncer_host="${pgbouncer_address%:*}"
pgbouncer_port="${pgbouncer_address##*:}"

if ! bash -c "cat < /dev/null > /dev/tcp/${pgbouncer_host}/${pgbouncer_port}" 2>/dev/null; then
  echo "PgBouncer is not reachable at ${pgbouncer_address}" >&2
  exit 1
fi

host="${pubsub_host%:*}"
port="${pubsub_host##*:}"

if ! bash -c "cat < /dev/null > /dev/tcp/${host}/${port}" 2>/dev/null; then
  echo "Pub/Sub emulator is not reachable at ${pubsub_host}" >&2
  exit 1
fi

clamd_host="${clamd_address%:*}"
clamd_port="${clamd_address##*:}"
clamd_response="$(
  exec 3<>"/dev/tcp/${clamd_host}/${clamd_port}"
  printf 'zPING\0' >&3
  head -c 5 <&3 | tr -d '\0'
)" || {
  echo "ClamAV is not reachable at ${clamd_address}" >&2
  exit 1
}

if [[ "${clamd_response}" != "PONG" ]]; then
  echo "ClamAV at ${clamd_address} did not respond to PING" >&2
  exit 1
fi

mkdir -p .local/storage

echo "local dependencies are ready"
