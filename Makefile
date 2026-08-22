.PHONY: help fmt fmt-check lint test check build up down health reset-local docker-clean pubsub-init migrate seed api worker dequeuer mcp image image-smoke image-push clamav-mirror

PROOFPLANE_CONFIG ?= .local/config.yaml

# Where `make image` records the reference it built, so the smoke and push steps
# operate on that exact image rather than re-deriving a tag. `.local/` is already
# ignored, and the file is disposable.
IMAGE_REF_FILE ?= .local/image-ref

# The migrate command's `lock_timeout` is a session setting, which a transaction
# pooler does not carry into the migration's own transactions. Production
# migrates through the database's direct endpoint, so this points at Postgres on
# 5432 rather than at PgBouncer on 6432.
PROOFPLANE_MIGRATION_DATABASE_URL ?= postgres://proofplane:proofplane@127.0.0.1:5432/proofplane

# The local stack serves no certificate. The migration command verifies the
# certificate chain and the hostname by default, so a local run has to lower it
# here. Production sets nothing and gets the verified default.
PROOFPLANE_MIGRATION_DATABASE_TLS ?= disable

help:
	@printf '%s\n' \
		'Targets:' \
		'  make fmt               Format Rust code' \
		'  make fmt-check         Check Rust formatting' \
		'  make lint              Run clippy with warnings denied' \
		'  make test              Run all tests (needs make up first)' \
		'  make check             Run fmt-check, lint, and test (needs make up first)' \
		'  make build             Build package' \
		'  make up                Start local Docker dependencies' \
		'  make down              Stop local Docker dependencies' \
		'  make health            Check local dependency readiness' \
		'  make reset-local       Destroy and recreate local dependency state' \
		'  make docker-clean      Remove leftover test containers and dangling volumes' \
		'  make pubsub-init       Create the local Pub/Sub emulator topics and subscription' \
		'  make migrate           Apply migrations, without seeding' \
		'  make seed              Apply migrations and seed local data' \
		'  make api               Run API binary' \
		'  make worker            Run worker binary' \
		'  make dequeuer          Run outbox dequeuer binary' \
		'  make mcp               Run MCP binary' \
		'  make image             Build the linux/amd64 production release image' \
		'  make image-smoke       Validate every command in the release image' \
		'  make image-push        Push the release image and print its digest' \
		'  make clamav-mirror     Mirror the pinned ClamAV image into Artifact Registry'

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

check: fmt-check lint test

build:
	cargo build

# The emulator keeps no state between runs, and no runtime process provisions
# anything, so the local topics and the worker subscription are created here.
up:
	docker compose up -d
	@bash scripts/init-local-pubsub.sh

down:
	docker compose down

health:
	bash scripts/check-local-deps.sh

reset-local:
	docker compose down -v --remove-orphans
	rm -rf .local/storage
	mkdir -p .local/storage/quarantine .local/storage/evidence
	docker compose up -d
	@bash scripts/init-local-pubsub.sh

# Each Postgres-backed test owns its container and `ContainerAsync`'s
# remove-on-drop clears it, so a test run that finishes leaves nothing behind.
# A run killed partway through does. This sweeps up after those.
docker-clean:
	@ids="$$(docker ps -aq --filter label=org.testcontainers.managed-by=testcontainers)"; \
	if [ -n "$$ids" ]; then \
		docker rm --force --volumes $$ids; \
	else \
		echo 'No leftover test containers.'; \
	fi
	@echo 'Pruning dangling volumes (machine-wide, not only Proofplane).'
	@docker volume prune --force

# Run again after changing the worker push endpoint in the local configuration.
pubsub-init:
	@bash scripts/init-local-pubsub.sh

migrate:
	@PROOFPLANE_MIGRATION_DATABASE_URL=$(PROOFPLANE_MIGRATION_DATABASE_URL) \
	 PROOFPLANE_MIGRATION_DATABASE_TLS=$(PROOFPLANE_MIGRATION_DATABASE_TLS) \
	 cargo run --quiet --bin migrate

seed:
	@PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --quiet --bin seed

api:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin api

worker:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin worker

dequeuer:
	RUST_LOG='info,proofplane=debug' PUBSUB_EMULATOR_HOST=127.0.0.1:8086 PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin dequeuer

mcp:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin mcp

# Release image targets. These build and publish from a workstation because
# there is no CI and Terraform builds nothing. See
# docs/runbooks/production-deployment.md.
image:
	@mkdir -p $(dir $(IMAGE_REF_FILE))
	@bash scripts/build-image.sh > $(IMAGE_REF_FILE).tmp
	@mv $(IMAGE_REF_FILE).tmp $(IMAGE_REF_FILE)
	@cat $(IMAGE_REF_FILE)

# Rebuilds first, so the checks never run against a stale reference from an
# earlier build. A rebuild costs seconds once the cargo cache mounts are warm.
image-smoke: image
	@bash scripts/smoke-image.sh "$$(cat $(IMAGE_REF_FILE))"

# Depends on the smoke checks so an unvalidated image cannot reach the registry.
image-push: image-smoke
	@bash scripts/push-image.sh "$$(cat $(IMAGE_REF_FILE))"

clamav-mirror:
	@bash scripts/mirror-clamav.sh
