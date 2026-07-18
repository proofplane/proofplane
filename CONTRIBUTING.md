# Contributing to Proofplane

Proofplane is a single Rust crate. Keep changes scoped, run the repository
checks before handing work off, and update local docs when a change alters setup
or workflow.

## Prerequisites

Install these tools before starting:

- Rust and Cargo for the current stable toolchain
- `make`
- Docker with Docker Compose
- [`ngrok`](https://ngrok.com/download) — only needed to connect a hosted agent
  (Claude/Cowork) to your local server
- An agent to connect: the [Codex CLI](https://developers.openai.com/codex/) or
  Claude/Cowork

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
storage directory when needed. `make seed` runs the database migrations and
writes demo data (an owner user, a workspace, frameworks, controls, and an
evidence request).

The local Docker services listen on:

- Postgres: `127.0.0.1:5432`
- Pub/Sub emulator: `127.0.0.1:8085`
- ClamAV clamd: `127.0.0.1:3310`

If you have an old local database from a previous schema, reset it:

```bash
make reset-local
make seed
```

## How authentication works

 MCP authenticates with an OAuth flow:

- Agents (Codex, Claude/Cowork) obtain a short-lived Proofplane PASETO access
  token via Authorization Code + PKCE, brokered by the Proofplane OAuth facade,
  with Auth0 as the upstream human login.
- The control-plane REST routes (`/me`, `/workspaces`, the OAuth endpoints, and
  browser document flows) authenticate with Auth0 user JWTs.

Because there is no static token, exercising MCP locally means completing the
OAuth flow with a real agent (or the MCP Inspector's OAuth mode). The setup
below gets you there. See the
[Agent Connector Onboarding spec](docs/epics/agent-connector-onboarding/spec.md)
for the full design.

## Configuration

Make targets default to the loopback config:

```text
PROOFPLANE_CONFIG=config/local.yaml
```

`config/local.yaml` is committed and covers process bind addresses, Postgres,
Pub/Sub, PASETO keys, filesystem object storage, ClamAV, observability, Auth0
settings, worker settings, upload limits, and health paths.

To connect a real agent you need public URLs and real Auth0 credentials, so use
a private override at `.local/config.yaml` (the whole `.local/` directory is
gitignored, so it is safe for secrets and per-session values):

```bash
cp config/local.yaml .local/config.yaml
```

Then run any process against it by exporting the path once:

```bash
export PROOFPLANE_CONFIG=.local/config.yaml
```

Object storage is not run in Docker Compose for the MVP; local config reserves
`.local/storage` for the filesystem-backed adapter. Production GCS work is
tracked in the
[Production Runtime Adapters epic](docs/epics/production-runtime-adapters/README.md).

## Running Processes

Start a process with the Make target for that binary (they read
`$PROOFPLANE_CONFIG`, defaulting to `config/local.yaml`):

```bash
make api        # control-plane REST + OAuth authorization server, :3000
make worker
make dequeuer
make mcp        # Streamable HTTP MCP data plane, :3002
```

## Connecting Codex or Cowork (single flow)

The OAuth authorization server (`:3000`) and the MCP data plane (`:3002`) are two
origins. Both must be reachable by the agent. Claude/Cowork is hosted, so it
needs public URLs; using ngrok for **both** agents gives one consistent setup.
(Codex is local — see the note at the end for a no-ngrok shortcut.)

### 1. Copy the config

```bash
cp config/local.yaml .local/config.yaml
```

Fill in the Auth0 upstream application credentials once (from the Proofplane dev
Auth0 tenant — ask a teammate if you don't have them):

```yaml
auth0:
  upstream_oauth:
    client_id: "<dev-auth0-app-client-id>"
    client_secret: "<dev-auth0-app-client-secret>"
```

### 2. Start ngrok

The committed `ngrok.yaml` at the repo root defines two endpoints (`api` → 3000,
`mcp` → 3002) with no authtoken. Provide the authtoken via the environment so
nothing secret is committed:

```bash
export NGROK_AUTHTOKEN=<your-ngrok-authtoken>
ngrok start --all --config ngrok.yaml
```

Read the two public URLs it assigned (they change on every restart):

```bash
curl -s 127.0.0.1:4040/api/tunnels | python3 -c '
import sys, json
t = {x["name"]: x["public_url"] for x in json.load(sys.stdin)["tunnels"]}
print("server.public_api_base_url :", t["api"])
print("mcp.resource        :", t["mcp"] + "/mcp")
print("allowed_hosts entry :", t["mcp"].split("://")[1])
print("agent connector URL :", t["mcp"] + "/mcp")'
```

### 3. Fill the ngrok values into `.local/config.yaml`

```yaml
server:
  public_api_base_url: "https://<api>.ngrok-free.app"   # the api URL
mcp:
  resource: "https://<mcp>.ngrok-free.app/mcp"           # the mcp URL + /mcp
  allowed_hosts:
    - "<mcp>.ngrok-free.app"                             # the mcp host, no scheme
```

`mcp.resource` must be **identical** to the URL you give the agent — the
`/oauth/authorize` resource check is exact. `allowed_hosts` is required because
the MCP transport rejects any `Host` it doesn't recognize (an empty list keeps
the localhost-only default).

### 4. Register the Auth0 callback

Add this **once** to the Auth0 upstream app's Allowed Callback URLs. If it's already
there you can skip this step. Use a wildcard so the rotating `api` URL always matches
and you never edit Auth0 again:

```text
https://*.ngrok-free.app/oauth/auth0/callback
```

### 5. Run the servers against `.local/config.yaml`

Both processes must use the same config, or the OAuth `issuer` won't match:

```bash
PROOFPLANE_CONFIG=.local/config.yaml make api
PROOFPLANE_CONFIG=.local/config.yaml make mcp
```

### 6. Connect the agent

**Codex** ([docs](https://developers.openai.com/codex/mcp)):

```bash
codex mcp add proofplane --url https://<mcp>.ngrok-free.app/mcp
codex mcp login proofplane
```

**Claude / Cowork:** add a custom connector by URL, using
`https://<mcp>.ngrok-free.app/mcp`. Claude registers itself via Dynamic Client
Registration; no client setup is needed.

Either agent then runs discovery → registration → Auth0 login → workspace
consent, and calls tools with a 24-hour token.

### Notes and gotchas

- **URLs rotate** every `ngrok` restart, so repeat steps 2–3 (and re-point the
  agent) each session. The Auth0 wildcard and `public_api_base_url`/`mcp.resource`
  are the only moving parts.
- **24-hour re-consent:** tokens last 24h and there is no refresh token yet, so
  the agent re-runs the browser flow daily. This is a known v1 limitation.

### MCP Inspector

The Inspector can connect through the same OAuth flow (Streamable HTTP, no static
token). Point it at your MCP URL and complete the browser authorization; use the
"Proxy" connection type to avoid CORS issues.

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
cargo test --test integration mcp
cargo test --test integration agent_connection_repository
cargo test --test integration evidence_submissions
```

## Repository Notes

- Application schema migrations live in `migrations/`.
- Application code lives under `src/`, grouped by route, service, repository,
  domain, and external adapter responsibility.
- MVP planning and tickets live in [`docs/epics/`](docs/epics/).
- The compliance data plane is exposed only through MCP; there is no REST data
  plane and no API tokens.
- MCP authenticates with the Proofplane OAuth facade (PASETO access tokens);
  control-plane REST routes authenticate with Auth0 user JWTs.
- Each user has exactly one workspace.

Do not commit credentials or production configuration. Keep private overrides and
secrets in the gitignored `.local/` directory; `ngrok.yaml` stays committed
because it holds no secret (the authtoken comes from `NGROK_AUTHTOKEN`).
