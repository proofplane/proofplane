#!/usr/bin/env bash
set -euo pipefail

pubsub_host="${PUBSUB_EMULATOR_HOST:-127.0.0.1:8085}"
spicedb_endpoint="${SPICEDB_ENDPOINT:-http://127.0.0.1:50051}"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker compose ps >/dev/null 2>&1; then
  echo "docker compose is not available from this directory" >&2
  exit 1
fi

docker compose exec -T postgres pg_isready -U proofplane -d proofplane >/dev/null

host="${pubsub_host%:*}"
port="${pubsub_host##*:}"

if ! bash -c "cat < /dev/null > /dev/tcp/${host}/${port}" 2>/dev/null; then
  echo "Pub/Sub emulator is not reachable at ${pubsub_host}" >&2
  exit 1
fi

spicedb_host_port="${spicedb_endpoint#*://}"
spicedb_host_port="${spicedb_host_port%%/*}"
spicedb_host="${spicedb_host_port%:*}"
spicedb_port="${spicedb_host_port##*:}"

if ! bash -c "cat < /dev/null > /dev/tcp/${spicedb_host}/${spicedb_port}" 2>/dev/null; then
  echo "SpiceDB is not reachable at ${spicedb_endpoint}" >&2
  exit 1
fi

mkdir -p .local/storage

echo "local dependencies are ready"
