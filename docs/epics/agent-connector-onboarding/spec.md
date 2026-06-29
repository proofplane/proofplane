# Agent Connector Onboarding Spec

## Goal

Let a non-technical operations or compliance user connect Proofplane to an AI
agent without installing a CLI, editing a configuration file, changing binary
permissions, or copying a long-lived API token.

The core product principle is **connect an account, not install a server**.
Proofplane remains a hosted Streamable HTTP MCP service. Claude/Cowork uses a
remote connector, Codex uses a plugin, and both continue through browser
authorization.

## Current Reality

The MCP runtime and core tools already use Streamable HTTP at `/mcp`. They
authenticate every transport request with a pre-provisioned `ppat_` bearer
token. The web UI can issue that token, but its setup preview describes a stdio
connection without a command or remote endpoint.

This works for technical API consumers but creates the wrong first-run path for
the target customer:

- the user must create, copy, and preserve a long-lived credential;
- each agent requires different manual configuration;
- credentials can be copied into plaintext configuration or shell state;
- Proofplane cannot present an account-level connection list or browser-led
  revocation flow;
- the setup language assumes the user understands MCP transports and clients.

Existing `ppat_` authentication remains supported for CI, unattended agents,
direct REST consumers, and MCP clients that cannot perform interactive OAuth.

## Product Boundary

This epic delivers:

- standards-compatible interactive authorization for the hosted MCP endpoint;
- a guided website flow that starts from the user's chosen agent;
- first-class Claude/Cowork and Codex distribution artifacts;
- generic remote-MCP instructions as a fallback;
- connection visibility, revocation, and attributable audit events.

This epic does not install or run a Proofplane server on the user's computer.
A universal macOS/Windows installer, Docker-based gateway, and Claude `.mcpb`
bundle are deferred unless a future local-resource use case requires them.
Cursor and VS Code distribution are explicitly outside this epic.

This epic also does not make the MCP server a web-asset host. Browser pages,
React assets, Auth0 callbacks, consent screens, and connection-management UI
remain owned by the existing website and API surface. The MCP service stays a
protocol/resource server.

## Target Journey

The default journey is:

1. The user selects Claude/Cowork, Codex, or another supported agent in
   Proofplane.
2. Proofplane opens the client's native add/install experience where one
   exists, or shows the shortest client-specific path.
3. The client discovers that the hosted MCP endpoint requires authorization.
4. A browser opens for Proofplane sign-in.
5. The user selects a workspace and reviews the requested access.
6. The client receives credentials without displaying them to the user or
   model.
7. Proofplane verifies the connection and offers a useful first prompt.

An experienced user may still copy the remote MCP URL or configure a `ppat_`
token manually.

## Remote MCP Authorization

The production MCP endpoint must be public HTTPS and implement the MCP
authorization contract for Streamable HTTP, including:

- Protected Resource Metadata discovery;
- authorization-server metadata discovery;
- OAuth 2.1 authorization code flow with PKCE;
- resource indicators and audience validation for the MCP endpoint;
- short-lived access tokens;
- refresh-token rotation when refresh tokens are issued;
- explicit revocation;
- generic authentication failures that do not reveal credential state.

Auth0 remains the human identity provider. Proofplane's website/API owns the
MCP authorization facade: browser login handoff, Auth0 callback handling,
workspace selection, consent, authorization-code issuance, token exchange,
refresh, and revocation. The MCP server must not serve the OAuth UI or static
web assets.

The MCP server publishes or participates in the metadata needed by MCP clients
to discover the authorization server, then validates MCP-scoped bearer
credentials on `/mcp`. It may verify self-contained tokens, call an
introspection endpoint, or load signing keys from the API-owned authorization
service, but its runtime responsibility is token verification and MCP
workspace/scope enforcement.

MCP access tokens are interactive connection credentials and must not be
represented to users as `ppat_` API tokens.

The website route that renders the authorization experience is
`/connect/mcp/authorize`. Protocol endpoints live under the API authorization
surface, for example `/oauth/authorize`, `/oauth/token`, `/oauth/revoke`, and
OAuth metadata routes. The API issues Proofplane OAuth authorization codes and
tokens; Auth0 does not directly issue MCP access or refresh tokens.

Authorization codes are opaque, one-time-use, PKCE-bound values stored only as
digests with a short TTL. The approval screen binds a grant to exactly one
user, one workspace, one OAuth client, one MCP resource, and one permission
set. A client that needs multiple workspaces must create separate connections.

The initial OAuth scope vocabulary is the existing workspace permission
vocabulary:

- `read_evidence_requests`
- `write_evidence_requests`
- `read_evidence_submissions`
- `write_evidence_submissions`
- `read_controls`
- `write_controls`

`offline_access` is accepted only to request a refresh token. It does not map
to a workspace permission. First-class agent flows may present a friendly
"manage SOC 2 workspace" preset, but persisted grants store the concrete
permission strings. Tools that require an ungranted permission return
authorization failure rather than escalating the connection.

Proofplane supports known OAuth clients only for this epic. First-class clients
such as Claude/Cowork and the Proofplane Codex plugin are pre-registered with
stable metadata, support status, and exact redirect-URI policy. Dynamic Client
Registration is explicitly deferred. Generic MCP clients that cannot use a
known OAuth client remain on the advanced API-token setup path until a later
epic adds generic OAuth registration.

MCP access and refresh tokens are opaque Proofplane-issued credentials stored
only as digests. Access tokens expire after 15 minutes. Refresh tokens rotate
on every use, have a 30-day idle lifetime, and have a 90-day absolute lifetime.
Refresh-token reuse revokes the token family and requires the user to connect
again. Revoking a connection immediately invalidates future refreshes and MCP
requests for that connection.

Credentials remain HTTP transport metadata. They must never appear in MCP tool
arguments, tool results, prompts, logs, analytics, URLs, or browser storage
owned by the UI.

## Connection Lifecycle And Audit

Proofplane must let a user see agent connections associated with their account,
including enough information to recognize the client, workspace, granted
access, creation time, and recent use. Users can revoke a connection without
revoking unrelated API tokens or other clients.

Authorization, refresh, use, rejection, and revocation produce structured audit
events using identifier-only fields. Tool-level audit behavior from the MCP
Server epic remains unchanged.

The persisted model has three logical records:

- known OAuth clients and their redirect-URI policy;
- agent connections, one per user/workspace/client/resource/permission-set
  grant;
- OAuth credentials, stored by digest and linked to an agent connection.

Connection records include client name, client type, workspace, granted
permissions, creation time, last authenticated MCP use, last refresh, and
revocation time. Raw access and refresh credentials must never be stored.

## Client Distribution

### Claude And Cowork

Proofplane ships as a remote connector usable from Claude, Cowork, and Claude
Desktop. The initial release may use a custom connector URL. Directory
submission material follows once production OAuth, support, privacy, tool
annotations, and test-account requirements are satisfied.

A local `.mcpb` extension is not the primary path because it is limited to
Claude Desktop and introduces an unnecessary local runtime.

### Codex

Proofplane ships a Codex plugin that bundles:

- the hosted MCP server definition;
- focused SOC 2 workflow skills and instructions;
- safe first prompts and approval guidance.

The plugin must not contain a customer credential. Authentication occurs
through the remote MCP authorization flow during installation or first use only
after that path is verified in the Codex app.

Current Codex documentation describes OAuth for Streamable HTTP MCP servers via
configuration plus `codex mcp login`. The observed Codex Desktop custom-MCP
form exposes Streamable HTTP URL and bearer-token configuration, but does not
show an inline OAuth connect action. Proofplane must therefore not treat the
desktop custom-MCP form as the non-technical Codex onboarding path unless that
surface gains OAuth support. Until plugin-driven OAuth is verified, direct
Codex MCP setup is an advanced path and may require CLI/config steps or a
bearer-token environment variable.

The initial Codex plugin is Proofplane-owned and named `proofplane-soc2`. It
ships through a Proofplane-controlled personal or repo marketplace during
development, then moves to broader distribution only after plugin-led OAuth is
validated. The plugin includes the hosted MCP declaration, SOC 2 workflow
skills, server/tool instructions, starter prompts, approval guidance, and
support/privacy/terms metadata. It must not be positioned as no-token
onboarding until install or first use can complete OAuth without manual
CLI/config work.

### Other Clients

Generic clients receive the remote MCP URL and advanced API-token setup
instructions. Generic OAuth setup is deferred until Dynamic Client
Registration or an equivalent known-client policy is added in a later epic.
Manual JSON/TOML and API-token setup remains an advanced fallback, not the
default call to action.

Generic guidance is generated from one versioned connection definition or
equivalent source so endpoint and naming changes do not drift between docs.

## Public Endpoint Configuration And Routing

Endpoint URLs are configuration, not code constants. OAuth resource, issuer,
redirect, and metadata values must be derived from validated public base URL
config so local, preview, and production environments can differ safely.

The required public URL roles are:

- MCP resource URL: the externally visible Streamable HTTP MCP endpoint, for
  example `http://127.0.0.1:3002/mcp` locally or
  `https://mcp.proofplane.com/mcp` in production;
- authorization issuer URL: the externally visible API/OAuth issuer, for
  example `http://127.0.0.1:3000` locally or
  `https://api.proofplane.com` in production;
- app base URL: the externally visible browser UI origin, for example
  `http://127.0.0.1:5173` locally or `https://app.proofplane.com` in
  production.

The MCP server serves Streamable HTTP protocol traffic at the configured MCP
resource URL and protected resource metadata for that resource. It advertises
the configured authorization issuer rather than serving browser UI itself.
Protocol endpoints and token exchange live under the API/OAuth issuer. Browser
UI and static assets live under the configured app base URL.

Every environment must validate these URLs at startup. The MCP resource URL
must include the `/mcp` path. The issuer and app base URLs must be absolute
origins without fragments. Production values must use HTTPS; local development
may use loopback HTTP.

The initial production MCP deployment may run as a single replica or use
ingress stickiness keyed by `Mcp-Session-Id`. The session identifier remains
protocol state only and never grants authorization. Horizontal scaling without
stickiness requires replacing the current process-local MCP session manager
with shared session state or a stateless-compatible transport strategy.

## Website Experience

Replace the current token-success MCP preview with an agent connection area
that:

- asks which agent the user uses;
- labels tested, preview, and generic clients honestly;
- launches the best available install/connect mechanism;
- shows authorization and connection progress;
- records successful authorization without claiming the external client is
  healthy;
- offers a first task such as reviewing open evidence requests;
- links advanced users to API-token setup separately.

All browser-facing OAuth and connection-management screens are served by the
website. If a client starts authorization from the MCP endpoint, discovery and
redirect metadata must route the browser to the website/API authorization
surface rather than to MCP-hosted HTML.

Proofplane owns authorization and revocation state. The agent harness owns its
own MCP connection health and should be the place where users see whether its
tools are mounted and callable.

After OAuth completes, Proofplane records the agent connection and may show it
as authorized. It must not block OAuth success while waiting for MCP traffic or
try to mirror the client's connection-health UI. Authenticated MCP traffic
updates `last_used_at` for audit/debugging only. If a connection is revoked or
credentials become unusable, Proofplane can show that the connection requires
reconnection.

## Compatibility And Failure Behavior

- An unsupported client receives generic remote-MCP guidance without an
  unsupported installer promise.
- A user without workspace access cannot authorize that workspace.
- Denied consent returns to a recoverable state and creates no usable grant.
- Revoked, expired, wrong-resource, and wrong-workspace credentials fail closed.
- Client-directory rejection does not block custom-connector or generic URL
  setup.
- Existing REST and MCP `ppat_` callers continue to work.

## Testing

- Protocol tests cover discovery metadata, PKCE, resource/audience validation,
  refresh rotation, denial, expiry, and revocation.
- Integration tests prove workspace isolation and coexistence with `ppat_`
  authentication.
- UI tests cover each client-path state without invoking third-party desktop
  applications.
- Release smoke tests exercise Claude/Cowork and Codex against a
  production-like endpoint.
- Distribution artifacts are validated against their host's current manifest
  or submission requirements.

## Open Decisions

Only one decision remains open before implementation:

- whether Codex app plugin installation can initiate and complete MCP OAuth
  without manual CLI/config work.

This is a validation spike, not a protocol-design blocker. If it fails, Codex
ships as an advanced documented path using `codex mcp login` or bearer-token
configuration while Claude/Cowork remains the first-class non-technical
connector path.

## Distribution Order And Submission Material

Ship and validate distribution in this order:

1. Production remote MCP plus website/API OAuth.
2. Claude/Cowork custom connector setup.
3. Guided website connection flow for Claude/Cowork.
4. Codex plugin preview through a Proofplane-controlled marketplace.
5. Directory or broader marketplace submission after production smoke tests.

Directory and marketplace submissions require privacy policy, terms of service,
support contact, security overview, OAuth scope descriptions, tool list,
screenshots, starter prompts, troubleshooting guide, and a durable test
workspace/account.

## Revisions

- 2026-06-29: Initial proposal based on research into hosted OAuth connectors,
  client directories and deep links, Claude MCP bundles, and multi-client MCP
  gateways. Chose hosted remote MCP plus native client distribution as the
  default and retained API tokens for technical automation.
- 2026-06-29: Excluded Cursor and VS Code distribution. Retained generic
  remote-MCP guidance only as a fallback beyond Claude/Cowork and Codex.
- 2026-06-29: Fixed the OAuth ownership boundary. The website/API owns the MCP
  authorization facade and all browser UI; the MCP server remains a
  protocol/resource server that publishes discovery metadata, verifies
  MCP-scoped credentials, and enforces workspace/scope access.
- 2026-06-29: Refined the Codex path after checking current Codex behavior.
  Codex documents OAuth-capable Streamable HTTP MCP via `codex mcp login`, but
  the desktop custom-MCP form appears bearer-token oriented. The epic now
  requires explicit validation before presenting Codex plugin installation as a
  no-token path.
- 2026-06-29: Resolved most open decisions. Chose a Proofplane API-owned OAuth
  facade backed by Auth0 identity, one connection per user/workspace/client,
  existing workspace permissions as scopes, known-client OAuth only, opaque
  rotating credentials, and configurable public endpoint roles.
- 2026-06-29: Clarified that MCP resource, authorization issuer, and app base
  URLs are environment configuration. Production hostnames are examples, not
  hard-coded requirements.
- 2026-06-29: Removed the website readiness taxonomy. Proofplane now owns
  authorization/revocation state and last-used audit metadata; the agent
  harness owns MCP connection-health display.
- 2026-06-29: Removed Dynamic Client Registration from MVP scope. Generic MCP
  clients stay on the advanced API-token path until a later generic OAuth
  registration epic.
