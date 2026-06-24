# Contributing to Proofplane

Proofplane is a single Rust crate. Keep changes scoped, run the repository
checks before handing work off, and update local docs when a change alters setup
or workflow.

## Prerequisites

Install these tools before starting:

- Rust and Cargo for the current stable toolchain
- `make`
- Docker with Docker Compose

## First Setup

From the repository root:

```bash
cargo build
make up
make health
make seed
```

`make up` starts local Postgres, the Pub/Sub emulator, and ClamAV. `make health`
checks that those services are reachable and creates the local filesystem
storage directory when needed.

The local Docker services listen on:

- Postgres: `127.0.0.1:5432`
- Pub/Sub emulator: `127.0.0.1:8085`
- ClamAV clamd: `127.0.0.1:3310`

`make seed` runs the application database migrations before writing local data.
The current schema is a consolidated initial schema. If you have an old local
database from before the API-token cutover, use:

```bash
make reset-local
make seed
```

The seed output prints a local owner bearer API token. Use that token for
data-plane API calls:

```bash
export PROOFPLANE_API_TOKEN=ppat_replace_with_latest_seed_output
curl --fail-with-body \
  --header "authorization: Bearer $PROOFPLANE_API_TOKEN" \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

Management routes still use Auth0 bearer JWTs. Local fixture examples for the
data plane live in [`fixtures/api/README.md`](fixtures/api/README.md).

## Configuration

Local Make targets default to:

```text
PROOFPLANE_CONFIG=config/local.yaml
```

Pass another config path when needed:

```bash
make api PROOFPLANE_CONFIG=path/to/config.yaml
```

The local config covers process bind addresses, Postgres, Pub/Sub, PASETO API
and download keys, filesystem object storage, ClamAV, observability, Auth0
settings, worker settings, upload limits, and health paths.

Object storage is not run in Docker Compose for the MVP. Local config reserves
`.local/storage` for the filesystem-backed object storage adapter. Production
GCS work is tracked in the
[Production Runtime Adapters epic](docs/epics/production-runtime-adapters/README.md).

## Running Processes

Start a process with the Make target for that binary:

```bash
make api
make worker
make dequeuer
make mcp
```

The local API listens on `127.0.0.1:3000` with the default config.

### Testing MCP With The Inspector

The local MCP server uses Streamable HTTP. Start the local
dependencies and seed data, then run the MCP server:

```bash
make up
make health
make seed
make mcp
```

Copy the fresh `ppat_...` token printed by `make seed` from the line:

```text
local owner bearer API token (reissued by this seed run): ppat_...
```

The seed command reissues this token each time it runs. If you rerun
`make seed`, update any MCP client or Inspector configuration with the new
token.

In another terminal, start the MCP Inspector:

```bash
npx @modelcontextprotocol/inspector
```

Open the Inspector URL printed by that command, including its proxy token query
parameter. Configure the connection as:

- Transport Type: `Streamable HTTP`
- URL: `http://127.0.0.1:3002/mcp`
- Connection Type: `Proxy`
- Custom header enabled:
  - Name: `Authorization`
  - Value: `Bearer ppat_...`

**Make sure you use the "Proxy" connection type.** It won't work with "Direct"
because of CORS.

Optionally, to verify the server and token outside the Inspector:

```bash
curl --fail-with-body -i http://127.0.0.1:3002/mcp \
  --header "Authorization: Bearer $PROOFPLANE_API_TOKEN" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"curl","version":"1.0"}}}'
```

Stop or reset local Docker state with:

```bash
make down
make reset-local
```

`make reset-local` destroys Docker volumes for the local dependency stack and
recreates the filesystem storage directory. Use it when local dependency state
needs to be rebuilt or when a schema-squashing change requires a fresh local
database.

## Validation

Run the standard repository check before submitting code:

```bash
make check
```

That runs:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

The integration tests use Docker-backed Testcontainers for Postgres and ClamAV,
so Docker must be available for the full test suite.

Useful focused commands:

```bash
cargo test --test integration request_auth
cargo test --test integration evidence_submissions
cargo test --test integration api_token
cargo test --test integration repository
```

## Repository Notes

- Application schema migrations live in `migrations/`.
- Application code lives under `src/`, grouped by route, service, repository,
  domain, and external adapter responsibility.
- MVP planning and tickets live in [`docs/epics/`](docs/epics/).
- Data-plane HTTP routes authenticate with user-owned PASETO API tokens.
- Management-plane routes authenticate with Auth0 bearer JWTs.

Do not commit credentials or production configuration. Use
`PROOFPLANE_CONFIG=path/to/config.yaml` for local overrides.
