# Contributing to Proofplane

This repository is an early Rust scaffold for Proofplane. Keep changes scoped,
run the repo checks before handing work off, and update the local docs when a
change alters setup or deployment order.

## Prerequisites

Install these tools before starting:

- Rust and Cargo for the current stable toolchain
- `make`
- Docker with Docker Compose
- the AuthZed `zed` CLI for SpiceDB schema validation

On macOS, install `zed` with Homebrew:

```bash
brew install authzed/tap/zed
```

The Rust build vendors `protoc`, so contributors do not need a separate local
Protocol Buffers compiler for the generated SpiceDB bindings.

## First Setup

From the repository root:

```bash
cargo build
make up
make health
make authz-schema-validate
make authz-schema
make seed
```

`make up` starts the local Postgres, Pub/Sub emulator, and SpiceDB services.
`make health` checks that those services are reachable and creates the local
filesystem storage directory when needed.

The local Docker services listen on:

- Postgres: `127.0.0.1:5432`
- Pub/Sub emulator: `127.0.0.1:8085`
- SpiceDB gRPC: `127.0.0.1:50051`
- ClamAV clamd: `127.0.0.1:3310`

SpiceDB stores its local state in the `proofplane_spicedb` database on the
local Postgres service. `make up` runs the idempotent database create step and
the SpiceDB datastore migration before it starts SpiceDB, so an existing local
Postgres volume can be reused.

SpiceDB schema deployment is explicit. Validate the schema fixture before
applying the configured schema, and apply it before seed writes authorization
relationships. The API does not deploy the SpiceDB schema on startup.

The seed binary runs the application database migrations before writing local
data. It expects the SpiceDB schema to exist before it writes the seeded local
workspace membership.

## Configuration

Local Make targets default to:

```text
PROOFPLANE_CONFIG=config/local.yaml
```

Pass another config path when needed:

```bash
make api PROOFPLANE_CONFIG=path/to/config.yaml
```

The local config covers process bind addresses, Postgres, Pub/Sub, SpiceDB,
object storage, observability, auth settings, worker settings, and health
paths. In particular, `spicedb.schema_path` selects the schema file used by
`make authz-schema`.

Object storage is not run in Docker Compose for the MVP. Local config reserves
`.local/storage` for the filesystem-backed object storage adapter. Production
GCS work is tracked in the
[Production Runtime Adapters epic](docs/epics/production-runtime-adapters/README.md).

## Running Processes

Start a process with the Make target for that binary:

```bash
make api
make worker
make mcp
```

The local API listens on `127.0.0.1:3000` with the default config.

Stop or reset local Docker state with:

```bash
make down
make reset-local
```

`make reset-local` destroys Docker volumes for the local dependency stack and
recreates the filesystem storage directory. Use it when local dependency state
needs to be rebuilt.

## Validation

Run the standard repository check before submitting code:

```bash
make check
```

That runs:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

The integration tests use Docker-backed Testcontainers for Postgres and
SpiceDB, so Docker must be available for the full test suite.

When editing SpiceDB schema or validation fixtures, also run:

```bash
make authz-schema-validate
```

## Repository Notes

- Application schema migrations live in `migrations/`.
- SpiceDB schema-as-code lives in `authz/spicedb/`.
- Architecture notes live in [`docs/architecture.md`](docs/architecture.md).
- MVP planning and tickets live in [`docs/epics/`](docs/epics/).

Do not add generated SpiceDB Rust types to application-facing interfaces.
Keep generated AuthZed protobuf types behind the authorization adapter.
