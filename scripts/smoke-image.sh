#!/usr/bin/env bash
set -euo pipefail

# Validates a built production image before anything pushes it. Cloud Run
# selects a binary by absolute path and supplies no arguments, so a renamed
# binary, a missing runtime library, or a host-architecture build all produce an
# image that only fails once it is deployed. These checks move that failure to
# the workstation. See docs/epics/production-deployment/spec.md.

image="${1:-}"

if [[ -z "${image}" ]]; then
  echo "usage: $0 <image-reference>" >&2
  exit 2
fi

# The five commands infra/gcp/production/03-release/run.tf overrides, and
# nothing else.
readonly commands=(api mcp worker dequeuer migrate)

# Local development commands. seed writes demo data, and pubsub-init provisions
# the local emulator. Neither may run against production.
readonly local_only_commands=(seed pubsub-init)

# Cloud Run runs linux/amd64. The operator's workstation is usually arm64.
readonly expected_platform="linux/amd64"
readonly expected_user="10001:10001"
readonly expected_uid="10001"

# The four runtimes read one YAML file named by this variable and have no
# default path, so an empty environment reaches this message from src/config.
readonly runtime_message="environment variable PROOFPLANE_CONFIG is required"

# migrate resolves its own credential and never loads the application
# configuration. See src/migrate.rs.
readonly migrate_message="no source is set"

failures=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

pass() {
  echo "ok:   $*"
}

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or is not on PATH" >&2
  exit 127
fi

if ! docker image inspect "${image}" >/dev/null 2>&1; then
  echo "image ${image} is not present locally: build it first" >&2
  exit 1
fi

echo "checking ${image}"

platform="$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "${image}")"

if [[ "${platform}" != "${expected_platform}" ]]; then
  fail "image is ${platform}, but Cloud Run requires ${expected_platform}"
else
  pass "platform is ${platform}"
fi

declared_user="$(docker image inspect --format '{{.Config.User}}' "${image}")"

if [[ "${declared_user}" != "${expected_user}" ]]; then
  fail "image declares USER '${declared_user:-root}', expected ${expected_user}"
else
  pass "image declares USER ${declared_user}"
fi

# A wrong architecture makes every remaining check fail for the same single
# reason. Report that reason once rather than five more times.
if [[ "${platform}" != "${expected_platform}" ]]; then
  echo >&2
  echo "stopping: the remaining checks run commands inside the image" >&2
  exit 1
fi

observed_uid="$(docker run --rm --platform "${expected_platform}" --entrypoint id "${image}" -u)"

if [[ "${observed_uid}" != "${expected_uid}" ]]; then
  fail "container runs as uid ${observed_uid}, expected ${expected_uid}"
else
  pass "container runs as uid ${observed_uid}"
fi

# reqwest and the database connector both verify TLS through the platform
# certificate store here. Without this bundle, the image cannot read the Auth0
# JWKS, cannot mint a GCP token, cannot publish to Pub/Sub, and cannot open a
# verified database connection.
if docker run --rm --platform "${expected_platform}" --entrypoint test "${image}" -r /etc/ssl/certs/ca-certificates.crt; then
  pass "CA certificate bundle is readable"
else
  fail "/etc/ssl/certs/ca-certificates.crt is missing or unreadable"
fi

# The release gates reject a production plan that can execute a local command,
# so those commands must not be in the image an operator deploys.
#
# `test` reports 1 for an absent file, and docker reports 125 to 127 when it
# cannot run the container at all. Require the 1, so a broken docker invocation
# cannot report this as a pass.
for name in "${local_only_commands[@]}"; do
  docker run --rm --platform "${expected_platform}" --entrypoint test "${image}" \
    -e "/usr/local/bin/${name}" && absent_status=0 || absent_status=$?

  case "${absent_status}" in
    0) fail "/usr/local/bin/${name} is present, but ${name} must never ship to production" ;;
    1) pass "${name} is absent" ;;
    *) fail "could not check for ${name}: docker exited ${absent_status}" ;;
  esac
done

for name in "${commands[@]}"; do
  path="/usr/local/bin/${name}"

  if ! docker run --rm --platform "${expected_platform}" --entrypoint test "${image}" -x "${path}"; then
    fail "${path} is missing or is not executable"
    continue
  fi

  case "${name}" in
    migrate) expected_message="${migrate_message}" ;;
    *) expected_message="${runtime_message}" ;;
  esac

  # An empty environment reaches each command's first documented failure. That
  # is what proves the command executes: the ELF loads, the dynamic libssl link
  # resolves, and main runs far enough to report why it cannot continue.
  #
  # --read-only proves the other half. Configuration arrives as a read-only
  # secret mount and evidence goes to object storage, so no command may need a
  # writable filesystem to start.
  output="$(docker run --rm --read-only --platform "${expected_platform}" \
    --entrypoint "${path}" "${image}" 2>&1)" && status=0 || status=$?

  if [[ "${status}" -ne 1 ]]; then
    fail "${path} exited ${status}, expected 1"
    continue
  fi

  if [[ "${output}" != *"${expected_message}"* ]]; then
    fail "${path} did not report '${expected_message}', got: ${output}"
    continue
  fi

  pass "${path} starts read-only and reports its missing credential"
done

echo

if [[ "${failures}" -ne 0 ]]; then
  echo "${failures} check(s) failed: ${image} is not releasable" >&2
  exit 1
fi

echo "${image} passed every check"
