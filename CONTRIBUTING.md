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
evidence). To apply migrations without any demo data — the same command
production runs — use `make migrate`.

The local Docker services listen on:

- Postgres: `127.0.0.1:5432`
- Pub/Sub emulator: `127.0.0.1:8086`
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
below gets you there.

## Configuration

Make targets default to the private local config:

```text
PROOFPLANE_CONFIG=.local/config.yaml
```

`config/local.yaml` is the committed template. It covers process bind addresses,
Postgres, Pub/Sub, PASETO keys, filesystem object storage, ClamAV,
observability, Auth0 settings, worker settings, upload limits, and health paths.
Copy it before running a process for the first time:

```bash
cp config/local.yaml .local/config.yaml
```

The whole `.local/` directory is gitignored, so `.local/config.yaml` is safe for
real Auth0 credentials, public tunnel URLs, and other per-session values. To use
a different config path, override the Make variable or export it once:

```bash
PROOFPLANE_CONFIG=path/to/config.yaml make api
# or
export PROOFPLANE_CONFIG=path/to/config.yaml
```

Object storage is not run in Docker Compose for the MVP; local config reserves
`.local/storage` for the filesystem-backed adapter. Production GCS work is
tracked in the
[Production Runtime Adapters epic](docs/epics/production-runtime-adapters/README.md).

## Running Processes

Runtime processes never apply migrations. After `make up`, and whenever the
embedded migration set changes, run `make migrate` or `make seed` before
starting a process. A process refuses to start when the database history is
behind its binary, or diverges from it, or runs ahead of it by any migration
that is not labeled `expand_`.

Start a process with the Make target for that binary (they read
`$PROOFPLANE_CONFIG`, defaulting to `.local/config.yaml`):

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

Run the standard repository check before submitting code, with the local stack
already running:

```bash
make up
make check
```

That runs:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

Docker is needed for the whole of `cargo test`, not just the integration-v2
binary, but the two test boundaries use it differently:

- The repository and application tests compiled into the library start a
  Postgres container per test through Testcontainers and remove it when the test
  finishes. They need a running Docker daemon and nothing else.
- The integration-v2 suite starts no containers. It uses the Postgres and deltio
  Pub/Sub emulator from `make up`, and fails at startup with a message naming
  the missing service if they are not running. It works in
  `proofplane_integration_v2`, a database it drops and recreates each run, so it
  never disturbs the `proofplane` database you develop against.

ClamAV behavior is provided by an in-process fake clamd server, so the compose
`clamav` service is not involved in the test suite.

Useful focused commands:

```bash
cargo test --test integration-v2 oauth
cargo test --test integration-v2 mcp::authentication
cargo test --test integration-v2 agent_connections
cargo test --test integration-v2 mcp::evidence
```

## Repository Notes

- Application schema migrations live in `migrations/`; see
  [`migrations/README.md`](migrations/README.md) for the naming convention and
  the expand-then-contract rule every schema change follows.
- Application code lives under `src/`, grouped by route, service, repository,
  domain, and external adapter responsibility.
- Epic specs live in [`docs/epics/`](docs/epics/); tickets are
  [GitHub issues](https://github.com/proofplane/proofplane/issues).
- The compliance data plane is exposed only through MCP; there is no REST data
  plane and no API tokens.
- MCP authenticates with the Proofplane OAuth facade (PASETO access tokens);
  control-plane REST routes authenticate with Auth0 user JWTs.
- Each user has exactly one workspace.

Do not commit credentials or production configuration. Keep private overrides and
secrets in the gitignored `.local/` directory; `ngrok.yaml` stays committed
because it holds no secret (the authtoken comes from `NGROK_AUTHTOKEN`).
