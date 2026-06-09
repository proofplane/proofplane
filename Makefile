.PHONY: help fmt fmt-check lint test check build up down health reset-local migrate authz-schema-validate authz-schema seed api worker dequeuer mcp

PROOFPLANE_CONFIG ?= config/local.yaml

help:
	@printf '%s\n' \
		'Targets:' \
		'  make fmt               Format Rust code' \
		'  make fmt-check         Check Rust formatting' \
		'  make lint              Run clippy with warnings denied' \
		'  make test              Run all tests' \
		'  make check             Run fmt-check, lint, and test' \
		'  make build             Build package' \
		'  make up                Start local Docker dependencies' \
		'  make down              Stop local Docker dependencies' \
		'  make health            Check local dependency readiness' \
		'  make reset-local       Destroy and recreate local dependency state' \
		'  make migrate           Run migrations' \
		'  make authz-schema-validate Validate SpiceDB schema fixtures with zed' \
		'  make authz-schema      Apply the configured SpiceDB schema' \
		'  make seed              Run seed binary' \
		'  make api               Run API binary' \
		'  make worker            Run worker binary' \
		'  make dequeuer          Run outbox dequeuer binary' \
		'  make mcp               Run MCP binary'

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

up:
	docker compose up -d

down:
	docker compose down

health:
	bash scripts/check-local-deps.sh

reset-local:
	docker compose down -v --remove-orphans
	rm -rf .local/storage
	mkdir -p .local/storage
	docker compose up -d

migrate:
	PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin seed

authz-schema-validate:
	zed validate authz/spicedb/proofplane.validation.yaml

authz-schema:
	@PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --quiet --bin authz-schema

seed:
	@PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --quiet --bin seed

api:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin api

worker:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin worker

dequeuer:
	RUST_LOG='info,proofplane=debug' PUBSUB_EMULATOR_HOST=127.0.0.1:8085 PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin dequeuer

mcp:
	RUST_LOG='info,proofplane=debug' PROOFPLANE_CONFIG=$(PROOFPLANE_CONFIG) cargo run --bin mcp
