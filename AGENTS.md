# Repository Guidelines

## Project Structure & Module Organization

Proofplane is a single Rust crate. Application code lives in `src/`, organized
by responsibility: HTTP endpoints in `routes/`, orchestration in `services/`,
persistence in `repository/` and `store/`, types in `domain/`, and external
adapters in `authentication`, `object_storage/`, `pubsub/`, and `scanner/`.
Executable entry points are in `src/bin/` (`api`, `worker`, `dequeuer`, `mcp`,
and `seed`).

Unit tests are colocated with source modules. Docker-backed integration tests
live under `tests/integration/`, with shared setup in `support.rs`. Database
migrations belong in `migrations/`. API fixtures and project design notes live
in `docs/`.

## Build, Test, and Development Commands

- `make build`: compile the package and generated bindings.
- `make check`: run formatting checks, Clippy with warnings denied, and all
  tests. Run this before submitting changes.
- `make up && make health`: start and verify local Postgres, Pub/Sub, and
  ClamAV dependencies.
- `make seed`: run database migrations and seed local data.
- `make api`, `make worker`, `make dequeuer`, or `make mcp`: run a specific
  process using `.local/config.yaml`. Copy `config/local.yaml` there for a fresh
  setup.
- `cargo test --test integration worker_handlers`: run a focused integration
  test module.

Docker must be available for integration tests because they use Testcontainers.

## Coding Style & Naming Conventions

Use standard Rust formatting (`cargo fmt`) and four-space indentation. Name
modules, functions, and tests in `snake_case`; types and traits in `PascalCase`;
constants in `SCREAMING_SNAKE_CASE`. Keep domain and application interfaces
independent of generated adapter types. Prefer existing concrete Postgres
gateways for internal persistence and traits for genuine external adapter
boundaries.

Never call `.expect(...)` in long-running server or runtime paths where a panic
could cause application downtime. This includes the API, MCP server, worker,
dequeuer, and shared library code reachable by those processes. Propagate
recoverable failures with `Result` and `?`, and handle invariants without a
panic path. Tests and one-shot utilities such as seed, migration, and local
development commands may use `expect` when aborting is the intended behavior
and the message is actionable. Before completing a change, search modified
runtime code for `.expect(` and remove every occurrence.

## Testing Guidelines

Use `#[test]` or `#[tokio::test]` unit tests for pure behavior. Put database,
transaction, HTTP, worker coordination, and dependency-boundary behavior in
`tests/integration/`. Name tests after observable outcomes, such as
`malicious_scan_marks_document_contains_virus`. There is no numeric coverage
threshold; changes should cover success, failure, and rollback paths appropriate
to their risk.

## Commit & Pull Request Guidelines

Recent commits use short, imperative summaries such as `Update outdated docs`.
Keep commits focused. Pull requests should explain behavior changes, identify
migrations or configuration effects, link the relevant issue or epic ticket, and
list validation commands run. Update `docs/` and fixtures when contracts or
setup steps change.

## Configuration & Security

Do not commit credentials or production configuration. Override the default
with `PROOFPLANE_CONFIG=path/to/config.yaml`. Treat `make reset-local` as
destructive: it removes Docker volumes and `.local/storage`.
