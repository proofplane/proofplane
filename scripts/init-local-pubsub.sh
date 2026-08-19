#!/usr/bin/env bash
set -euo pipefail

# Creates the topics and the worker push subscription in the local Pub/Sub
# emulator. `make up` runs this because the emulator keeps no state between
# runs, and because no runtime process provisions anything any more: Terraform
# owns the production resources. See docs/epics/production-runtime-adapters.

pubsub_host="${PUBSUB_EMULATOR_HOST:-127.0.0.1:8086}"
host="${pubsub_host%:*}"
port="${pubsub_host##*:}"

# The compose services are started detached, so the emulator may still be
# opening its port. deltio needs about a second.
attempt=1
until bash -c "cat < /dev/null > /dev/tcp/${host}/${port}" 2>/dev/null; do
  if [[ "${attempt}" -ge 20 ]]; then
    echo "Pub/Sub emulator is not reachable at ${pubsub_host}" >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 0.5
done

# The runtime configuration names the project, the subscription, and the push
# endpoint. A fresh checkout has no copy yet, so fall back to the template it is
# copied from. Both name the same local emulator resources.
config_path="${PROOFPLANE_CONFIG:-.local/config.yaml}"

if [[ ! -f "${config_path}" ]]; then
  config_path="config/local.yaml"
fi

PROOFPLANE_CONFIG="${config_path}" PUBSUB_EMULATOR_HOST="${pubsub_host}" \
  cargo run --quiet --bin pubsub-init
