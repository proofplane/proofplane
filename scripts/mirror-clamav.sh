#!/usr/bin/env bash
set -euo pipefail

# Mirrors the pinned upstream ClamAV image into the production Artifact Registry
# repository, so a release depends on neither Docker Hub availability nor a
# mutable tag. Prints the mirrored @sha256 reference on stdout.
#
# This mirror is not deployable on its own.
# infra/gcp/production/03-release/run.tf consumes two ClamAV images, and
# neither is stock upstream: the worker sidecar has to
# copy the last-good definition snapshot out of GCS and run with freshclam
# disabled, and the update job needs an image of its own. Both derived images
# belong to #121. This script provides the pinned base they start from.

# Matches the clamav service in docker-compose.yml. The digest is the
# multi-platform index, resolved from Docker Hub. The tag travels with it only
# so the Artifact Registry cleanup policy does not treat the mirror as untagged.
source_image="clamav/clamav-debian"
source_tag="1.4.3"
source_digest="sha256:1d261f9c83ef2bbeef3915c7792f125e5707500fde0019d53547461747b90374"

project_id="${PROOFPLANE_PROJECT_ID:-}"

# Fixed, because infra/gcp/production/01-artifacts/variables.tf validates the
# region to us-central1 alone and names the repository this mirrors into.
region="us-central1"
repository="proofplane"
mirror_name="clamav"

if [[ -z "${project_id}" ]]; then
  echo "PROOFPLANE_PROJECT_ID is required: it names the production GCP project" >&2
  exit 2
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker buildx version >/dev/null 2>&1; then
  echo "docker buildx is not available, and the mirror needs imagetools" >&2
  exit 1
fi

source_reference="${source_image}:${source_tag}@${source_digest}"
remote="${region}-docker.pkg.dev/${project_id}/${repository}/${mirror_name}"

# Verify the pin still resolves before writing anything. An upstream tag that no
# longer carries this digest means the pin needs a deliberate review, not a
# silent mirror of whatever is there now.
if ! docker buildx imagetools inspect "${source_reference}" >/dev/null 2>&1; then
  echo "cannot resolve ${source_reference} upstream" >&2
  echo "the pinned digest may have been withdrawn: review it before mirroring" >&2
  exit 1
fi

echo "mirroring ${source_reference} to ${remote}:${source_tag}" >&2

# imagetools copies the manifest between registries without pulling every layer
# through the local daemon, and it preserves the multi-platform index.
docker buildx imagetools create \
  --tag "${remote}:${source_tag}" \
  "${source_reference}" >&2

digest="$(docker buildx imagetools inspect "${remote}:${source_tag}" --format '{{.Manifest.Digest}}')"

if [[ ! "${digest}" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "resolved '${digest}', which is not a sha256 digest" >&2
  exit 1
fi

# imagetools copies a single source index without change, so the mirror should
# carry the pinned digest. Report a difference rather than fail on it: the
# content came from a pinned source either way, and only the encoding of the
# index could differ.
if [[ "${digest}" != "${source_digest}" ]]; then
  echo "note: the mirror resolved to ${digest}, not the pinned ${source_digest}" >&2
  echo "the content still came from the pinned source, but review the difference" >&2
fi

echo "${remote}@${digest}"
