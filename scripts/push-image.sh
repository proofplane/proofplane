#!/usr/bin/env bash
set -euo pipefail

# Pushes a locally built release image to Artifact Registry and prints the
# immutable reference Terraform requires.
# infra/gcp/production/03-release/variables.tf validates
# ^[^[:space:]]+@sha256:[0-9a-f]{64}$, so a tag is not deployable and this
# script's stdout is the value an operator copies into
# 03-release/tfvars/production.tfvars.

project_id="${PROOFPLANE_PROJECT_ID:-}"

# Fixed, because infra/gcp/production/01-artifacts/variables.tf validates the
# region to us-central1 alone and names the repository this pushes into.
region="us-central1"
repository="proofplane"
image_name="proofplane"

local_image="${1:-}"

if [[ -z "${local_image}" ]]; then
  echo "usage: $0 <local-image-reference>" >&2
  exit 2
fi

if [[ -z "${project_id}" ]]; then
  echo "PROOFPLANE_PROJECT_ID is required: it names the production GCP project" >&2
  exit 2
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker image inspect "${local_image}" >/dev/null 2>&1; then
  echo "image ${local_image} is not present locally: build it first" >&2
  exit 1
fi

# Cheap re-assertion, because this script can be called without the smoke step.
platform="$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "${local_image}")"

if [[ "${platform}" != "linux/amd64" ]]; then
  echo "refusing to push ${local_image}: it is ${platform}, and Cloud Run requires linux/amd64" >&2
  exit 1
fi

# A digest reference or a bare name would both make the substitution below
# produce a wrong tag, so require a name:tag reference.
if [[ ! "${local_image}" =~ ^[^@]+:[^:/@]+$ ]]; then
  echo "refusing to push '${local_image}': expected a name:tag reference" >&2
  exit 1
fi

tag="${local_image##*:}"

# build-image.sh marks an image built from uncommitted work. The digest is the
# only record of what a release contains, so that image must not reach the
# registry an operator deploys from.
if [[ "${tag}" == *-dirty ]]; then
  echo "refusing to push ${local_image}: it was built from an unclean worktree" >&2
  echo "commit the work and run make image again" >&2
  exit 1
fi

remote="${region}-docker.pkg.dev/${project_id}/${repository}/${image_name}"

# The tag is not decoration. infra/gcp/production/01-artifacts/artifacts.tf
# runs a live cleanup policy that deletes untagged versions older than 30 days,
# so a digest-only push can expire out from under a rollback.
echo "pushing ${remote}:${tag}" >&2

docker tag "${local_image}" "${remote}:${tag}"
docker push "${remote}:${tag}" >&2

# Ask the registry rather than the local daemon: the digest Terraform deploys is
# the one the registry stored.
digest="$(docker buildx imagetools inspect "${remote}:${tag}" --format '{{.Manifest.Digest}}')"

if [[ ! "${digest}" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "resolved '${digest}', which is not a sha256 digest" >&2
  exit 1
fi

echo "${remote}@${digest}"
