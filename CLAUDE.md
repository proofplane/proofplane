# Repository Guidelines

## Project Structure & Module Organization

Proofplane is a single Rust crate. Application code lives in `src/`, organized
by responsibility: HTTP endpoints in `routes/`, orchestration in `services/`,
persistence in `repository/` and `store/`, types in `domain/`, and external
adapters in `authorization/`, `object_storage/`, `pubsub/`, and `scanner/`.
Executable entry points are in
`src/bin/` (`api`, `worker`, `dequeuer`, `mcp`, `seed`, and `authz-schema`).

Unit tests are colocated with source modules. Docker-backed integration tests
live under `tests/integration/`, with shared setup in `support.rs`. Database
migrations belong in `migrations/`; SpiceDB schema files belong in
`authz/spicedb/`. API fixtures and project design notes live in `docs/`.

## Build, Test, and Development Commands

- `make build`: compile the package and generated bindings.
- `make check`: run formatting checks, Clippy with warnings denied, and all
  tests. Run this before submitting changes.
- `make up && make health`: start and verify local Postgres, Pub/Sub, and
  SpiceDB dependencies.
- `make authz-schema && make seed`: apply authorization schema, run database
  migrations, and seed local data.
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

## Agent skills

### Issue tracker

Work is tracked in local Markdown tickets under `docs/epics/<epic>/tickets/`.
See `docs/agents/issue-tracker.md`.

### Triage labels

Tickets retain their `Todo`/`Done` lifecycle status and use a separate five-role
triage field. See `docs/agents/triage-labels.md`.

### Domain docs

Proofplane uses a single-context domain layout. See `docs/agents/domain.md`.
