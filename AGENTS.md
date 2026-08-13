# Repository Guidelines

## Project Structure & Module Organization

Proofplane is a single Rust crate. Application code lives in `src/`, organized
by responsibility: HTTP endpoints in `routes/`, orchestration in `services/`,
persistence in `persistence/`, types in `domain/` and `read_models/`, and external
adapters in `authentication/`, `object_storage/`, `pubsub/`, and `scanner/`.
Executable entry points are in `src/bin/` (`api`, `worker`, `dequeuer`, `mcp`,
and `seed`).

Database migrations belong in `migrations/`. API fixtures and project design
notes live in `docs/`.

## Build, Test, and Development Commands

- `make build`: compile the package and generated bindings.
- `make check`: run formatting checks, Clippy with warnings denied, and all
  tests. Run `make up` first: the integration-v2 suite uses the compose stack
  rather than starting services of its own.
- `make up && make health`: start and verify local Postgres, PgBouncer, Pub/Sub,
  and ClamAV dependencies.
- `make seed`: run database migrations and seed local data.
- `make api`, `make worker`, `make dequeuer`, or `make mcp`: run a specific
  process using `.local/config.yaml`. Copy `config/local.yaml` there for a fresh
  setup.
- `cargo test --test integration-v2 evidence_document_uploads`: run a focused
  integration-v2 test module.

## Architecture: Snapshot CQRS

Proofplane uses CQRS with complete aggregate snapshots in one Postgres database;
it does not use event sourcing or a separate read database. Commands mutate
aggregates inside a `UnitOfWork`. Queries load purpose-built read models directly
and must not rehydrate mutable aggregates or open write transactions. A command
may use transaction-scoped read gateways for authorization, eligibility,
relationships, or a response that must observe its save before commit.

Commands and queries are immutable, task-oriented values with concrete,
operation-specific handlers and inherent `handle` methods. Do not introduce
handler marker traits, a mediator, runtime registry, dynamic dispatcher, service
locator, or a generic repository abstraction. Routes, MCP tools, workers, and
services receive typed handlers from the composition root; coordinators remain
only when an operation also owns an external boundary such as tokens, object
storage, scanning, Pub/Sub, or identity.

Keep lifecycle transitions and invariants in domain aggregates. Application
handlers own authorization, tenant boundaries, parent eligibility, relationship
checks, and orchestration.

Aggregate transitions may return immutable, past-tense domain events, but only
emit an event when a real consumer translates it into audit work, an outbox
message, or a follow-up command. Rejected transitions and idempotent replays emit
no event. Persist an aggregate snapshot and its resulting outbox messages in the
same transaction. Integration messages distinguish imperative commands from
completed-fact events and remain safe for at-least-once delivery through
idempotent aggregate transitions.

## Coding Style & Naming Conventions

Use standard Rust formatting (`cargo fmt`) and four-space indentation. Name
modules, functions, and tests in `snake_case`; types and traits in `PascalCase`;
constants in `SCREAMING_SNAKE_CASE`. Keep generated and transport types out of
domain and application interfaces. Prefer existing concrete Postgres gateways
for internal persistence and traits for genuine external adapter boundaries.

Name persistence values for their concrete role: use `unit_of_work` for
`UnitOfWork`, `workspace` for `WorkspaceUnitOfWork`, `reads` for read gateway
collections, and `repository` or an aggregate-specific name such as
`policy_repository` for individual aggregate repositories. Do not use generic
`context` names for persistence values; reserve `context` for genuine
authentication, request, audit, and workflow context objects.

Never call `.expect(...)` in long-running server or runtime paths where a panic
could cause application downtime. This includes the API, MCP server, worker,
dequeuer, and shared library code reachable by those processes. Propagate
recoverable failures with `Result` and `?`, and handle invariants without a
panic path. Tests and one-shot utilities such as seed, migration, and local
development commands may use `expect` when aborting is the intended behavior
and the message is actionable. Before completing a change, search modified
runtime code for `.expect(` and remove every occurrence.

Repositories map aggregates through private persistence records and expose
narrow `get` and `save` operations: `get` rehydrates the complete aggregate,
and `save` persists its complete current snapshot. Snapshot `save` methods must
not add aggregate-specific authorization, eligibility, or relationship queries.

Name each aggregate's private primary persistence record after the aggregate,
and give it explicit `try_from_row(&Row)`, `from_domain(&Domain)`, and
`into_domain(...)` methods. Repository methods load rows into records and bind
records to SQL; they must not construct aggregates directly from rows or bind
domain fields directly in save SQL. Persist primary and companion records with
the shared full-snapshot upsert, which updates every non-conflict column.

Keep multi-table orchestration inside the owning repository. Synchronize
optional companion records to the aggregate snapshot. Replace owned child
collections by deleting the aggregate's current rows and inserting the complete
current collection in the same transaction. Workspace filtering belongs in
`get` and read operations; `save` trusts the handler's completed orchestration.

Name aggregate roots with the bare ubiquitous-language noun, such as `Control`
or `Evidence`; do not use an `Aggregate` suffix. Read-only shapes live under
`src/read_models/` and use names that describe their role, such as `ControlDetail`
or `PolicyCatalogEntry`. Aggregate repositories return aggregates only. Lists,
details, summaries, reverse mappings, and other read shapes must be loaded by a
dedicated read gateway. Reserve “projection” for a process that constructs or
maintains derived read-side state.

## Testing Guidelines

Write or update a test when a change creates or alters observable behavior, fixes
a bug, changes a domain invariant, authorization or concealment rule, public
contract, persistence mapping, transaction boundary, concurrency behavior,
idempotency rule, retry policy, or application-owned dependency failure. Cover
success, rejection, and rollback paths in proportion to risk. Prefer extending
the nearest existing test over repeating the same assertion at several layers;
there is no numeric coverage target.

Do not add tests for documentation-only changes, formatting, renames,
compiler-enforced facts, trivial getters or delegation, unchanged mechanical
refactors, or behavior owned by a third-party dependency. Do not pin private
implementation details when a stable behavioral boundary proves the same rule.
If behavior is unchanged, add a test only when the refactor crosses a risky
boundary or closes a demonstrated coverage gap.

Run `make check` before submitting implementation changes. For documentation-only
changes, run `git diff --check` and review links and referenced paths instead of
the Rust test suite.

Choose the narrowest boundary that proves the behavior without mocking the code
that owns it:

- Use colocated `#[test]` or `#[tokio::test]` tests for pure domain,
  serialization, validation, authorization, and other deterministic behavior.
- Use colocated tests backed by `persistence::test_support` and real Postgres for
  repository round trips, workspace scoping, locks, complete snapshots,
  constraints, transaction rollback, and application behavior that no client
  can observe directly.
- Use `tests/integration-v2/` for client-visible HTTP, MCP, browser, worker,
  Pub/Sub, scanner, object-storage, and end-to-end transaction behavior. Name
  tests after observable outcomes, such as
  `malicious_scan_marks_document_contains_virus`.

Both of those boundaries need Docker, in different ways. Tests backed by
`persistence::test_support` start a Postgres container of their own per test and
remove it when the test ends, so they need only a working Docker daemon. The
integration-v2 suite starts nothing: it uses the Postgres and Pub/Sub emulator
from the compose stack, so `make up` has to have been run first, and it drops
and recreates its own `proofplane_integration_v2` database on every run rather
than touching the `proofplane` database you develop against.

The compose stack also runs PgBouncer in transaction mode on 6432, standing in
for the Supavisor transaction pooler that production runtime traffic goes
through. The integration-v2 application under test connects through it, so the
suite proves pooler compatibility on every run; only the suite's own
`DROP DATABASE`/`CREATE DATABASE` reaches Postgres directly on 5432, because
neither can run through a transaction pooler. `config/local.yaml` points at 6432
too, so `make api` and friends exercise the same path.

Because of that pooler, **never call `query`, `query_one`, `query_opt`, or
`execute`.** Those name the prepared statement they create, and a transaction
pooler reassigns the server connection between the `Parse` and the `Bind`, so
the statement fails with `prepared statement "sN" does not exist`. Use
`query_typed`, `query_typed_one`, `query_typed_opt`, and `execute_typed`, which
parse into the unnamed statement and send the whole exchange under one `Sync`.

Those methods want each parameter's Postgres type. Do not write `Type::…` at a
call site: wrap the value in `persistence::param`, which recovers the type from
the Rust type through the `PgParam` trait in `src/persistence/params.rs`.

```rust
.query_typed_opt("SELECT id FROM users WHERE auth0_sub = $1", &[param(&auth0_sub)])
```

Aggregate saves need nothing extra — `snapshot_record!` derives each column's
type from the declared field type. Adding a `PgParam` impl means committing to a
column type, so check `migrations/` first; a Rust type with no impl is a compile
error, which is the point. Transactions are for atomicity, not for the pooler.

The integration-v2 suite is black-box: no database handle, no in-process
services, no request helpers on `TestApp`, and setup arranged inline in the test
body. Assert complete positive outcomes rather than guessed absence. **Read
`tests/integration-v2/README.md` before adding to it.** If a behavior cannot be
reached by a real client, test it at the appropriate lower boundary and record
that reason in the pull request.

## Commit & Pull Request Guidelines

Recent commits use short, imperative summaries such as `Update outdated docs`.
Keep commits focused. Pull requests should explain behavior changes, identify
migrations or configuration effects, link the relevant GitHub issue, and list
validation commands run. Update `docs/` and fixtures when contracts or setup
steps change.

## Configuration & Security

Do not commit credentials or production configuration. Override the default
with `PROOFPLANE_CONFIG=path/to/config.yaml`. Treat `make reset-local` as
destructive: it removes Docker volumes and `.local/storage`.

Treat `make docker-clean` as destructive too. It removes leftover Testcontainers
containers, then prunes dangling volumes machine-wide, which is not limited to
Proofplane. The local dev database survives while the compose stack is running or
merely stopped, but `make down` removes those containers and leaves
`proofplane_proofplane-postgres-data` dangling, so running `make docker-clean`
after `make down` deletes it. A normal test run cleans up after itself now, so
reach for this only after a run was killed partway through.

## Agent skills

### Issue tracker

Work is tracked as GitHub issues in `proofplane/proofplane`: one `Epic: <Name>`
issue per effort, with its tickets attached as sub-issues. Epic specs stay in the
repository under `docs/epics/<epic>/spec.md`. See `docs/agents/issue-tracker.md`.

### Triage labels

Implementation status is the issue's open/closed state. Triage is a separate
five-role GitHub label. See `docs/agents/triage-labels.md`.

### Domain docs

Proofplane uses a single-context domain layout. See `docs/agents/domain.md`.
