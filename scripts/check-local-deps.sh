#!/usr/bin/env bash
set -euo pipefail

pubsub_host="${PUBSUB_EMULATOR_HOST:-127.0.0.1:8085}"

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

mkdir -p .local/storage

echo "local dependencies are ready"
