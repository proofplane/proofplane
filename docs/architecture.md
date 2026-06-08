# Proofplane Architecture Notes

This document captures architectural decisions that should guide future work.
It is intentionally practical: keep it aligned with the code as the system
evolves.

## API Runtime

The API binary owns runtime assembly and process lifecycle. It should stay as
the place where infrastructure dependencies are created, startup tasks run, and
the HTTP server is launched.

The expected API startup flow is:

1. Load configuration from `PROOFPLANE_CONFIG`.
2. Initialize structured logging through `observability`.
3. Open a one-off Postgres connection for startup work.
4. Run database migrations before serving traffic.
5. Build the long-lived Postgres connection pool.
6. Wrap the pool in `repository::Postgres`.
7. Initialize metrics.
8. Bind the TCP listener.
9. Build the Axum app from `app::AppDependencies`.
10. Serve with graceful shutdown.

The binary should compose dependencies explicitly, then hand them to the app
layer. HTTP route modules should not load config, initialize tracing, run
migrations, or construct database pools.

## Worker Runtime

The dequeuer is a standalone outbox publisher. It loads config, initializes tracing,
runs migrations, builds a small Postgres pool, provisions Pub/Sub topics and the
worker push subscription, builds a `GoogleCloudPublisher`, and polls due
`outbox_messages`. When a row publishes successfully, the dequeuer deletes it.
When publishing fails, it records the failure, increments the attempt count, and
schedules the next retry using bounded backoff.

The worker is a separate HTTP runtime for Pub/Sub push delivery. It loads config,
initializes tracing, runs migrations, builds its own Postgres pool, installs
metrics, binds `server.worker_bind`, and serves `/pubsub/messages` plus health
and metrics routes. Pub/Sub push messages are acknowledged with `204 No Content`
for malformed or unknown events so they do not retry forever. Known retryable
handler failures return `500` so Pub/Sub can redeliver according to the
subscription policy.

## General Notes on Binaries

All binaries should also own their startup orchestration: load config,
initialize tracing, run required startup database work, then enter their runtime
or command behavior.

Shared database startup utilities belong in `store`, including direct
connections, connection pools, and migrations. Seed data is different: it is
owned by the `seed` binary because seeding is a maintenance command, not shared
infrastructure for application code.

## Module Boundaries

### `src/bin/api.rs`

`api.rs` is the API process entry point. It owns:

- process startup and shutdown;
- config loading;
- tracing initialization;
- startup database connection and migrations;
- long-lived pool construction;
- repository construction;
- metrics recorder installation;
- TCP listener binding;
- final `axum::serve` call.

The preferred shape is a small `main()` that delegates to `run()`.

### `src/store`

`store` is the low-level database infrastructure layer. It owns direct
integration with database libraries:

- `tokio_postgres` connections;
- `deadpool_postgres` pools;
- `refinery` migrations.

This module may know about connection strings, Postgres clients, pools, and
migration runners. Higher layers should not duplicate this setup logic.

### `src/repository`

`repository` is the application-facing database gateway. It wraps lower-level
pool types and provides methods that route and service code can depend on.

The current Postgres repository owns a `deadpool_postgres::Pool` internally.
It exposes:

- `get()` for direct pool access needed by infrastructure checks like
  readiness;
- `get_client()` for future repository operations that should use a wrapped
  repository client.

As product behavior grows, query methods and transaction helpers should live
here or in submodules below this boundary. Route handlers should avoid reaching
through to `store` for normal application data access.

Repository methods are persistence primitives, not authorization boundaries.
This matters for global identity data: actors and API credentials are not owned
by a single workspace, so repository methods may read or mutate them globally.
Do not expose those methods directly through HTTP routes. Any future actor or
API credential management API must authorize the requested management action
explicitly, for example with actor-owned credential permissions or an
organization/admin-level permission in SpiceDB.

### `src/app.rs`

`app` is HTTP composition. It owns construction of the root Axum `Router`.

`create_app` should take a single `AppDependencies` struct. This keeps app
construction stable as new routers and dependencies are added.

The app layer owns:

- nesting route modules under their configured paths;
- building per-router state from `AppDependencies`;
- attaching root HTTP middleware, including request logging.

It should not create infrastructure dependencies. Those are built by the binary
and passed in.

### `src/routes`

`routes` owns HTTP endpoint behavior. Each route module should expose a router
constructor and define the state required by that router.

Routes may depend on already-constructed application dependencies, such as
`Arc<repository::Postgres>` or a metrics handle. They should not load config,
run migrations, construct pools, or initialize global process state.

Route errors should map to stable HTTP responses in `routes::error`.

### `src/dequeuer`

`dequeuer` owns the transactional outbox polling loop. It should stay independent
from HTTP concerns. Its inputs are a `repository::Postgres`, a Pub/Sub
`Publisher`, and an `OutboxDequeuerConfig`.

The dequeuer is responsible for:

- listing due outbox rows;
- converting rows into Pub/Sub messages;
- deleting rows after successful publish;
- recording publish failures and retry timestamps;
- applying bounded retry backoff.

It should not know about individual domain handlers. Domain-specific work starts
after Pub/Sub delivers the message to the worker.

### `src/worker.rs` and `src/handlers`

`worker.rs` owns the Pub/Sub push HTTP surface and event dispatch. It builds the
worker router, decodes Pub/Sub push envelopes, validates message
data, maps malformed or unknown events to acknowledgements, and maps retryable
handler failures to `500`.

Domain-specific worker behavior belongs under `src/handlers`. The worker
dispatch layer should multiplex by event type and call the appropriate handler.

### `src/pubsub`

`pubsub` owns Pub/Sub integration. It defines application topic names, outbound
message publishing, Google Pub/Sub publisher construction, and worker
subscription provisioning. The dequeuer depends on this module for publishing and
startup provisioning; handlers should not publish directly unless a later domain
flow explicitly needs a new event.

## Dependency Direction

The API dependency direction should remain:

```text
src/bin/api.rs
  -> app::create_app(AppDependencies)
  -> routes::{health, metrics, version, error}
  -> repository
  -> store
```

The asynchronous worker dependency direction should remain:

```text
src/bin/dequeuer.rs
  -> dequeuer::OutboxDequeuer
  -> pubsub::Publisher
  -> repository
  -> store

src/bin/worker.rs
  -> worker::create_worker_app(WorkerAppDependencies)
  -> worker::router
  -> handlers
  -> repository
  -> store
```

The binary is allowed to depend on every layer because it assembles the process.
Routes and app code should depend only on the dependencies they are handed.
Lower layers should not depend on HTTP modules.

## Configuration

Postgres configuration is a connection string stored as a `SecretString`.
This keeps the runtime interface simple for `tokio_postgres` and avoids
spreading host, port, database, username, and password fields through the app.

The connection string may be exposed only at infrastructure boundaries that need
to pass it into database libraries.

## Observability

Proofplane uses `tracing_subscriber` for structured logging. We are not building
OpenTelemetry tracing, distributed traces, or span-exporter infrastructure at
this stage.

HTTP request logging belongs on the root router in `app`, so all routes receive
consistent request logs.

## Practical Rules

- Keep startup orchestration in binaries.
- Keep database library setup in `store`.
- Keep app-facing database access in `repository`.
- Keep HTTP composition in `app`.
- Keep endpoint behavior in `routes`.
- Keep outbox publishing in the standalone `dequeuer`.
- Keep Pub/Sub push decoding and event dispatch in `worker`.
- Pass dependencies explicitly through structs instead of reaching for globals.
- Prefer a single dependency struct at app construction boundaries.
- Do not add placeholder abstractions before a real boundary needs them.
