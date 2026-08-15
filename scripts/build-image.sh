#!/usr/bin/env bash
set -euo pipefail

# Builds the production release image. An operator builds a release on a
# workstation. There is no CI, and Terraform runs no build of its own. See
# docs/epics/production-deployment/spec.md.
#
# Prints the built image reference on stdout, so a caller can pass it to the
# smoke and push steps.

# Fixed, because infra/gcp/production/terraform.tfvars.example names exactly one
# image path for the release.
image_name="proofplane"
platform="linux/amd64"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker buildx version >/dev/null 2>&1; then
  echo "docker buildx is not available, and the release build needs it for ${platform}" >&2
  exit 1
fi

revision="$(git rev-parse --short HEAD)"

# awk rather than `sed | head`, because `head` closes the pipe early and
# `pipefail` then aborts the build on the resulting SIGPIPE.
version="$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)"

if [[ -z "${version}" ]]; then
  echo "could not read the package version from Cargo.toml" >&2
  exit 1
fi

# The runbook opens by requiring a clean intended checkout. An image built from
# uncommitted work cannot be reproduced from the digest it deploys under, and
# the digest is the only thing Terraform records.
if [[ -n "$(git status --porcelain)" ]]; then
  if [[ "${ALLOW_DIRTY:-0}" != "1" ]]; then
    echo "the worktree has uncommitted changes, so this image would not be reproducible" >&2
    echo "commit first, or set ALLOW_DIRTY=1 to build a throwaway image" >&2
    exit 1
  fi

  tag="${revision}-dirty"
  echo "warning: building from a dirty worktree; ${image_name}:${tag} is not releasable" >&2
else
  tag="${revision}"
fi

image="${image_name}:${tag}"

echo "building ${image} for ${platform} from ${revision}" >&2

docker buildx build \
  --platform "${platform}" \
  --load \
  --tag "${image}" \
  --build-arg GIT_REVISION="${revision}" \
  --build-arg VERSION="${version}" \
  "${repo_root}" >&2

echo "${image}"
