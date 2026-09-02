# syntax=docker/dockerfile:1
# check=skip=FromPlatformFlagConstDisallowed

# BuildKit warns that a constant --platform makes an image that cannot follow
# the build platform. That is the intent here, so the check is skipped above.
# Cloud Run accepts only linux/amd64, and this image has one correct target.

# One image carries every production command. Each Cloud Run resource in
# infra/gcp/production/run.tf overrides the command instead of selecting a
# process-specific image, so the binary paths below are a deployment contract
# rather than a convention.

# Cloud Run runs linux/amd64. An operator builds a release on a workstation that
# is usually arm64. The platform stays in the file rather than in a --platform
# flag, so a plain `docker build` cannot produce an image Cloud Run refuses.
FROM --platform=linux/amd64 rust:1.95-bookworm AS builder

# The rust image is buildpack-deps based, so pkg-config and libssl-dev are
# already here for openssl-sys, which jwtk and refinery both pull in and which
# links dynamically. aws-lc-sys drives its own build through cmake, which is not.
RUN apt-get update \
 && apt-get install --yes --no-install-recommends cmake \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# migrations/ is a build input, not a runtime one: src/persistence/migrate.rs
# calls embed_migrations!("./migrations"), which reads the directory when the
# crate compiles.
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

# The image omits seed. The release gates reject a production plan that can
# execute it, and the surest way to honor that gate is to omit the command.
#
# The cache mounts make the emulated amd64 build slow once rather than slow on
# every release. They also mean target/ does not survive this layer, so this
# step installs the binaries out of it.
#
# `set -e` is explicit because a RUN runs under /bin/sh without it. A loop
# reports the status of its last command, so an early install failure would
# otherwise produce an image that is missing a command.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    set -eu; \
    cargo build --release --locked \
      --bin api \
      --bin mcp \
      --bin worker \
      --bin dequeuer \
      --bin migrate; \
    mkdir -p /out; \
    for name in api mcp worker dequeuer migrate; do \
      install -m 0755 "target/release/${name}" "/out/${name}"; \
      strip "/out/${name}"; \
    done

FROM --platform=linux/amd64 debian:bookworm-slim AS runtime

# ca-certificates: reqwest and the database connector both verify TLS through
# the platform certificate store. Without it the image cannot read the Auth0
# JWKS, cannot mint a GCP token, cannot publish to Pub/Sub, and cannot open a
# verified database connection. libssl3: the openssl-sys link above is dynamic,
# which is also why a scratch or distroless-static runtime is not available here.
RUN apt-get update \
 && apt-get install --yes --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --user-group --no-create-home --shell /usr/sbin/nologin proofplane

COPY --from=builder /out/ /usr/local/bin/

# Nothing writes to the filesystem: configuration arrives as a read-only secret
# mount and evidence goes to object storage.
USER 10001:10001

# No ENTRYPOINT or CMD. Every Cloud Run resource sets its command explicitly, so
# a default here could only mask a missing override.

# Declared last on purpose. An ARG invalidates every layer after it, so keeping
# these at the end means a new commit rebuilds only the label.
ARG GIT_REVISION=unknown
ARG VERSION=unknown

LABEL org.opencontainers.image.title="proofplane" \
      org.opencontainers.image.description="Proofplane production commands: api, mcp, worker, dequeuer, migrate" \
      org.opencontainers.image.source="https://github.com/proofplane/proofplane" \
      org.opencontainers.image.revision="${GIT_REVISION}" \
      org.opencontainers.image.version="${VERSION}"
