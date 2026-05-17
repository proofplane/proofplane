.PHONY: help fmt fmt-check lint test test-integration check build up down migrate seed api worker mcp

PROOFPLANE_ENV ?= config/local.yaml

help:
	@printf '%s\n' \
		'Targets:' \
		'  make fmt               Format Rust code' \
		'  make fmt-check         Check Rust formatting' \
		'  make lint              Run clippy with warnings denied' \
		'  make test              Run all tests' \
		'  make test-integration  Run integration test target' \
		'  make check             Run fmt-check, lint, and test' \
		'  make build             Build package' \
		'  make up                Start local Docker dependencies' \
		'  make down              Stop local Docker dependencies' \
		'  make migrate           Run migrations' \
		'  make seed              Run seed binary' \
		'  make api               Run API binary' \
		'  make worker            Run worker binary' \
		'  make mcp               Run MCP binary'

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

test-integration:
	cargo test --test integration

check: fmt-check lint test

build:
	cargo build

up:
	docker compose up -d

down:
	docker compose down

migrate:
	PROOFPLANE_ENV=$(PROOFPLANE_ENV) cargo run --bin seed

seed:
	PROOFPLANE_ENV=$(PROOFPLANE_ENV) cargo run --bin seed

api:
	PROOFPLANE_ENV=$(PROOFPLANE_ENV) cargo run --bin api

worker:
	PROOFPLANE_ENV=$(PROOFPLANE_ENV) cargo run --bin worker

mcp:
	PROOFPLANE_ENV=$(PROOFPLANE_ENV) cargo run --bin mcp
