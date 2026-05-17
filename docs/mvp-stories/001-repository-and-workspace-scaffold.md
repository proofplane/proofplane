# 001 - Repository and Crate Scaffold

## Goal

Create the Rust crate layout that all later MVP work will build on.

## Design

Use a single Cargo package named `proofplane` with multiple binary entrypoints. Keep crate-internal modules under `src/` until there is a concrete reason to split packages. The scaffold should compile and expose small smoke-testable placeholders rather than full product behavior.

Dedicated binaries:

- `src/bin/api.rs`, invoked with `cargo run --bin api`
- `src/bin/worker.rs`, invoked with `cargo run --bin worker`
- `src/bin/mcp.rs`, invoked with `cargo run --bin mcp`
- `src/bin/seed.rs`, invoked with `cargo run --bin seed`

Use internal modules for:

- domain types
- application services
- repository traits and starter in-memory implementations
- Pub/Sub value types
- object storage traits and a filesystem-backed local implementation
- configuration
- observability
- validation
- errors and retry utilities

Suggested initial layout:

```text
proofplane/
  Cargo.toml
  Makefile
  docker-compose.yml
  migrations/
    V001__initial_schema.sql
  src/
    lib.rs
    bin/
      api.rs
      worker.rs
      mcp.rs
      seed.rs
    api/
    worker/
    mcp/
    domain/
    services/
    repositories/
    pubsub/
    storage/
    config/
    observability/
    validation/
    errors/
    migrations/
  tests/
    integration/
      main.rs
```

The integration suite should be a dedicated Cargo test target backed by `tests/integration/main.rs`, configured in `Cargo.toml` with `[[test]]`. Early integration tests should avoid external dependency assumptions and can exercise local implementations such as the filesystem object store.

## Acceptance Criteria

- Package builds with `cargo build`.
- API, worker, MCP server, and seed script exist as separate binaries addressable through `cargo run --bin api`, `cargo run --bin worker`, `cargo run --bin mcp`, and `cargo run --bin seed`.
- Internal modules compile even if they initially contain only skeletal types.
- Module boundaries make it possible to inject dependencies with traits and generic parameters.
- No runtime feature requires dynamic dispatch.
- Repository includes standard formatting, linting, test, local dependency, migration, seed, and binary run commands in a documented `Makefile`.
- `config/local.yaml` defines local defaults used by the `Makefile`.

## Tests

- Add smoke unit tests for the initial internal modules.
- Add a repository-level CI-equivalent command that runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- Add an integration test target under `tests/integration/main.rs` that compiles and runs without external services.

## QA Guide

1. Run `make build`.
2. Run `make check`.
3. Run `make test-integration`.
4. Run `cargo run --bin api`, `cargo run --bin worker`, `cargo run --bin mcp`, and `cargo run --bin seed` and confirm each prints its scaffold startup message.
5. Confirm the integration test target uses `tests/integration/main.rs`.
