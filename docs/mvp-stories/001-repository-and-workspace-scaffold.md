# 001 - Repository and Crate Scaffold

## Goal

Create the Rust crate layout that all later MVP work will build on.

## Design

Use a single Cargo package named `proofplane` with multiple binary entrypoints. Keep crate-internal modules under `src/` until there is a concrete reason to split packages. The scaffold should compile and expose small smoke-testable placeholders rather than full product behavior.

Dedicated binaries:

- `src/bin/api.rs`, invoked with `cargo run --bin api`
- `src/bin/dequeuer.rs`, invoked with `cargo run --bin dequeuer`
- `src/bin/mcp.rs`, invoked with `cargo run --bin mcp`
- `src/bin/seed.rs`, invoked with `cargo run --bin seed`

Use internal modules for:

- domain types
- application services
- repository and store scaffolding
- Pub/Sub value types
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
    app.rs
    bin/
      api.rs
      dequeuer.rs
      mcp.rs
      seed.rs
    routes/
    worker/
    mcp/
    domain/
    services/
    repository/
    store/
    pubsub/
    config/
    observability/
    validation/
    errors/
    migrations/
```

The dedicated integration suite is deferred to story 009. The initial scaffold should rely on unit tests and compile checks until real process and infrastructure behavior needs integration coverage.

## Acceptance Criteria

- Package builds with `cargo build`.
- API, dequeuer, MCP server, and seed script exist as separate binaries addressable through `cargo run --bin api`, `cargo run --bin dequeuer`, `cargo run --bin mcp`, and `cargo run --bin seed`.
- Internal modules compile even if they initially contain only skeletal types.
- Module boundaries make it possible to inject dependencies with traits and generic parameters.
- No runtime feature requires dynamic dispatch.
- Repository includes standard formatting, linting, test, local dependency, migration, seed, and binary run commands in a documented `Makefile`.
- `config/local.yaml` defines local defaults used by the `Makefile`.

## Tests

- Add smoke unit tests for the initial internal modules.
- Add a repository-level CI-equivalent command that runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- Defer the integration test target to story 009.

## QA Guide

1. Run `make build`.
2. Run `make check`.
3. Run `cargo run --bin api`, `cargo run --bin dequeuer`, `cargo run --bin mcp`, and `cargo run --bin seed` with local config and dependencies as needed.
