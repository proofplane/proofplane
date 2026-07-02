# Agent Connector Onboarding Spec

## Goal

Let a non-technical operations or compliance user connect Proofplane to an AI
agent without installing a CLI, editing configuration, or copying a long-lived
API token.

The core product principle is **connect an account, not install a server**.
Proofplane remains a hosted Streamable HTTP MCP service. Compatible clients
discover Auth0, complete browser authorization, and receive credentials
without exposing them to the user or model.

## Architecture Decision

Auth0 is the OAuth authorization server for remote MCP clients, following
[Auth0's direct MCP authorization flow](https://auth0.com/ai/docs/mcp/intro/why-auth-for-mcp).
Auth0 owns:

- authorization-server discovery;
- client registration and redirect-URI validation;
- Authorization Code with PKCE;
- login and OAuth consent;
- access-token issuance;
- signing-key publication and rotation;
- user-grant management; and
- OAuth protocol errors.

Proofplane does not implement `/oauth/authorize`, `/oauth/token`,
authorization-code storage, or a token-signing keyring. The initial release
does not request `offline_access` and Auth0 does not issue an MCP refresh token.
Auth0 access tokens expire after eight hours.

Proofplane remains responsible for:

- MCP Protected Resource Metadata and `401` challenges;
- selecting and approving one Proofplane workspace during authorization;
- durable agent-connection and audit records;
- Auth0 access-token validation;
- live workspace membership, scope, and revocation enforcement; and
- the hosted MCP tools and application experience.

The workspace grant is integrated into Auth0 through a post-login Redirect
Action. The Action pauses Auth0's transaction, sends the browser to a
Proofplane workspace-consent page, resumes at Auth0, and adds the approved
Proofplane connection and workspace identifiers to the Auth0 access token.

This replaces the earlier proposal for a Proofplane-owned OAuth facade and
Proofplane-issued PASETO credentials. The reason is operational and security
simplicity: Auth0 already implements the security-sensitive OAuth server
behavior required by MCP.

## Current Reality

The MCP runtime and core tools already use Streamable HTTP at `/mcp`. They
authenticate requests with a pre-provisioned, workspace-bound `ppat_` bearer
token. The website can issue that token, but this is the wrong default for
non-technical users:

- the user must create, copy, and preserve a long-lived credential;
- credentials can leak through plaintext configuration or shell state;
- clients require different manual setup;
- browser-led consent and reconnection are unavailable; and
- connection lifecycle and audit attribution are token-centric.

Existing `ppat_` authentication remains supported for CI, unattended agents,
direct REST consumers, and MCP clients without interactive OAuth.

## Product Boundary

This epic delivers:

- Auth0-backed OAuth authorization for the hosted MCP endpoint;
- one-workspace, scoped agent connections;
- a guided website flow for supported clients;
- first-class Claude/Cowork and Codex distribution artifacts;
- generic remote-MCP instructions as a fallback; and
- connection visibility, revocation, and attributable audit events.

This epic does not:

- run a local Proofplane server;
- implement an OAuth authorization server in the Proofplane API;
- synchronize every Proofplane workspace into Auth0 Organizations;
- pass MCP credentials through prompts, tools, logs, or browser storage;
- enable open Dynamic Client Registration in production; or
- support unattended OAuth connections that must outlive the access token; or
- remove existing API-token authentication.

Auth0 Organizations are not used for workspace binding. Auth0 currently
documents Organization user flows as
[unavailable for third-party applications](https://auth0.com/docs/get-started/applications/first-party-and-third-party-applications),
while MCP clients are third-party applications. A Proofplane Redirect Action
bridge provides workspace selection without depending on that unsupported
combination.

## Roles And Trust Boundaries

| Role | Component | Responsibility |
| --- | --- | --- |
| Resource owner | Proofplane user | Approves client access to one workspace |
| OAuth client | Claude, Codex, or another MCP harness | Holds credentials and calls MCP |
| Authorization server | Auth0 | Runs OAuth, login, consent, and access-token issuance |
| Resource server | Proofplane MCP | Validates Auth0 tokens and serves authorized tools |
| Domain authorization service | Proofplane API and database | Owns users, workspaces, grants, and revocation |

The MCP client is not the model. Credentials remain transport metadata held by
the client and must never enter model context.

```mermaid
flowchart LR
    U[Human user]
    C[MCP client<br/>Claude, Codex, or Inspector]
    I[Auth0<br/>OAuth authorization server]

    subgraph P[Proofplane]
        M[MCP resource server<br/>/mcp]
        W[Workspace consent UI]
        A[Grant and connection API]
        D[(Postgres)]
    end

    U -->|uses| C
    C -->|discover, authorize, and exchange code| I
    I -->|temporary Redirect Action| W
    U -->|selects workspace and approves| W
    W -->|create workspace grant| A
    A --> D
    W -->|resume transaction| I
    I -->|Eight-hour Auth0 access token| C
    C -->|Auth0 bearer access token| M
    M -->|live grant and membership check| D
```

## Target Journey

1. The user chooses Claude/Cowork, Codex, or another supported agent.
2. Proofplane opens the client's native connection path where one exists.
3. The client contacts the hosted MCP endpoint and receives an OAuth
   challenge.
4. The client discovers Auth0 and opens Auth0 Universal Login.
5. Auth0 authenticates the user and pauses at the Proofplane Redirect Action.
6. Proofplane shows the requested permissions and lets the user choose one
   accessible workspace.
7. Auth0 completes the authorization code flow and returns an eight-hour
   access token to the MCP client.
8. The client calls `/mcp`; Proofplane verifies the Auth0 token and the live
   workspace grant.
9. Proofplane records successful use and offers a useful first prompt.
10. After token expiry, the client starts authorization again. The Action
    reuses the one active workspace connection without showing the workspace
    page when it can do so safely.

The user never sees or copies the access token. If the client cannot restart
authorization automatically, the user reconnects it manually.

## Initial Authorization Flow

```mermaid
sequenceDiagram
    autonumber
    actor U as User
    participant C as MCP client
    participant M as Proofplane MCP
    participant I as Auth0
    participant X as Auth0 Post-Login Action
    participant W as Proofplane consent UI/API
    participant D as Postgres

    C->>M: Request /mcp without a token
    M-->>C: 401 + WWW-Authenticate resource_metadata
    C->>M: GET protected-resource metadata
    M-->>C: resource URI, Auth0 issuer, supported scopes
    C->>I: GET authorization-server metadata
    I-->>C: Auth0 authorize, token, registration, and PKCE metadata

    C->>C: Generate state and PKCE verifier/challenge
    C->>I: /authorize with client_id, redirect_uri, resource, scopes, challenge
    I->>U: Universal Login
    U->>I: Authenticate
    I->>X: Run post-login Action for the MCP resource

    X->>D: Find active connection for user, client, and resource
    D-->>X: No active connection
    X-->>W: Redirect with short-lived signed transaction token and Auth0 state
    W->>W: Verify signature, expiry, state, resource, client, user, and scopes
    W->>D: Load Proofplane user and accessible workspaces
    W-->>U: Show client, requested permissions, and workspace picker
    U->>W: Approve one workspace
    W->>D: Recheck membership and create short-lived pending connection
    W-->>I: POST signed connection result to /continue with original state

    I->>X: Resume onContinuePostLogin
    X->>X: Validate result and add connection/workspace claims
    I->>U: Show mandatory third-party OAuth consent when required
    U->>I: Approve requested MCP scopes
    I-->>C: Authorization code at registered redirect URI
    C->>I: Exchange code and PKCE verifier
    I-->>C: Eight-hour Auth0 access token

    C->>M: /mcp with Auth0 bearer access token
    M->>D: Validate and activate connection, membership, and scopes
    M-->>C: Authorized MCP response
```

### Discovery

The MCP server returns `401 Unauthorized` with:

```http
WWW-Authenticate: Bearer resource_metadata="https://mcp.example/.well-known/oauth-protected-resource/mcp"
```

The protected-resource metadata identifies:

- `resource`: the canonical public MCP URI;
- `authorization_servers`: the Auth0 issuer; and
- `scopes_supported`: the minimal MCP permission set.

The MCP server owns this endpoint under
[RFC 9728](https://datatracker.ietf.org/doc/html/rfc9728). Auth0 owns
authorization-server metadata under
[RFC 8414](https://datatracker.ietf.org/doc/html/rfc8414).

The Auth0 tenant must enable the Resource Parameter Compatibility Profile so
the MCP `resource` parameter becomes the access-token audience as required by
[RFC 8707](https://datatracker.ietf.org/doc/html/rfc8707).

### Client Registration

Production first-class clients use Auth0's recommended manual Client ID
Metadata Document registration where supported. Manual third-party
application registration is the fallback. Auth0, not Proofplane, stores client
IDs, redirect URIs, grant types, and token-endpoint authentication policy.

Known clients must:

- be third-party applications;
- use Authorization Code with PKCE;
- use OAuth access tokens without depending on third-party OIDC support;
- use exact registered redirect URIs;
- be granted access only to the Proofplane MCP resource and approved scopes;
- use the `authorization_code` grant without `offline_access`; and
- expose stable client identity suitable for connection display and audit.

Open Dynamic Client Registration is deferred. Auth0's DCR endpoint is
unauthenticated when enabled and requires tenant ACL and default third-party
permission policy. Generic clients remain on API-token setup until that policy
is deliberately shipped.

### Auth0 Resource Server

The Auth0 tenant defines one API/resource server whose identifier exactly
matches the canonical public MCP resource URI, for example:

```text
https://mcp.proofplane.com/mcp
```

It uses:

- RS256 access tokens and the RFC 9068 authorization token dialect;
- the concrete MCP scopes defined below;
- an eight-hour access-token lifetime;
- offline access disabled for the MCP resource; and
- domain-level identity connections usable by third-party clients.

The MCP runtime receives only public verification material through Auth0 JWKS.
Proofplane never receives Auth0 signing keys.

## Workspace Grant Bridge

Auth0 does not know Proofplane workspace membership. A post-login Redirect
Action integrates that domain decision without taking over OAuth.

### Initial authorization and connection reuse

Proofplane permits at most one active connection for an Auth0
user/client/resource tuple. The selected workspace is a property of that
connection. Connecting the same client to another workspace requires revoking
or replacing the existing connection through a visible user flow.

For every authorization transaction targeting the MCP resource, the Action:

1. verifies the expected Auth0 resource identifier and allowed client policy;
2. reads `sub`, `client_id`, requested scopes, resource, and transaction state;
3. asks Proofplane for the one active connection matching
   `sub`/`client_id`/resource;
4. when an active connection exists and the requested scopes are allowed,
   rechecks membership and adds its connection and workspace claims without a
   Proofplane browser redirect;
5. when no active connection exists, creates a short-lived signed redirect
   token and sends the browser to the workspace-consent route;
6. resumes only with the original Auth0 `state` and a signed Proofplane result;
7. verifies that result and rechecks it through the Proofplane grant API; and
8. adds namespaced connection and workspace claims to the access token.

An authorization request using `prompt=none` may succeed only through step 4.
If workspace selection, reconnection, or any other interaction is required,
the Action fails with `interaction_required` instead of attempting a redirect.
The client must then start a visible authorization flow.

The consent endpoint must not trust a workspace, scope, client, or user value
submitted by the browser. It verifies the Auth0-signed transaction, loads the
user by Auth0 subject, intersects requested scopes with the allowed MCP
vocabulary, checks current membership, and writes a short-lived pending
connection transactionally.

The workspace step occurs before Auth0 has necessarily recorded its mandatory
third-party consent or issued an authorization code. A pending connection is
therefore not shown as authorized. The first valid Auth0-backed MCP request
atomically activates it. If Auth0 consent is denied, code exchange is
abandoned, or no valid request arrives before the pending deadline, the record
expires and is removed. This prevents workspace approval from being mistaken
for completed OAuth authorization.

The continuation token is short-lived, single-use, bound to Auth0 state, and
contains only identifiers. Proofplane stores its digest or nonce to prevent
replay.

### Access-token expiry and reauthorization

Auth0 issues no MCP refresh token. When the eight-hour access token expires,
the MCP server returns `401 Unauthorized` and the client must start
Authorization Code with PKCE again.

The best case is nearly silent:

1. the client automatically restarts authorization;
2. the Auth0 browser session is still active;
3. the Action finds exactly one active Proofplane connection; and
4. Auth0 returns a new code without login or workspace interaction.

If the Auth0 session has expired, the user signs in again. If the connection
was revoked, replaced, or is otherwise unavailable, the user completes the
workspace step again. If the client does not automatically restart OAuth, its
tools remain disconnected until the user selects its Authenticate or
Reconnect control.

Automatic reauthorization is client behavior, not guaranteed by MCP. The
Claude/Cowork and Codex release gates must verify their behavior on access-token
expiry. This release does not support background or unattended OAuth work
beyond the token lifetime; those callers use `ppat_` credentials.

## Access Token Contract

The MCP server accepts only Auth0 access JWTs with all required standard and
Proofplane claims:

| Claim | Meaning |
| --- | --- |
| `iss` | Exact configured Auth0 issuer |
| `aud` | Canonical Proofplane MCP resource URI |
| `sub` | Auth0 user subject |
| `azp` | Authorized MCP client ID |
| `exp`, `iat` | Token lifetime |
| `scope` | Auth0-approved MCP scopes |
| `https://proofplane.com/connection_id` | Durable Proofplane agent connection |
| `https://proofplane.com/workspace_id` | Workspace selected during consent |

The claim namespace is configuration and must be collision-resistant. Tokens
contain identifiers and scopes only, never Auth0 credentials, user content, or
workspace data.

The `scope` claim and persisted connection permissions must agree exactly.
The resource server never trusts custom claims without the Auth0 signature,
issuer, audience, and lifetime checks.

## Runtime Authorization

```mermaid
flowchart TD
    R[Request to /mcp] --> B{Bearer credential present?}
    B -- no --> U[401 with resource metadata]
    B -- yes --> T{ppat_ token format?}
    T -- yes --> P[Existing API-token authenticator]
    T -- no --> J[Verify Auth0 JWT signature and registered claims]
    J --> C{Required MCP claims valid?}
    C -- no --> U
    C -- yes --> G[Load connection, user, workspace, and membership]
    G --> V{Claims match active grant and required scope?}
    V -- no --> U
    V -- yes --> A[Attach unified actor context]
    A --> M[Invoke MCP tool]
```

For every Auth0-backed MCP request, Proofplane:

1. verifies the JWT signature against cached Auth0 JWKS;
2. validates `iss`, `aud`, `exp`, `sub`, `azp`, and required custom claims;
3. loads the named pending or active connection;
4. requires its user, workspace, client, resource, and permissions to match;
5. atomically activates a valid, unexpired pending connection or requires an
   active connection, and requires membership to remain active;
6. checks the tool's required scope; and
7. records `last_used_at` and an agent-connection audit actor.

The database check makes revocation and membership removal immediate even
while an Auth0 JWT remains cryptographically valid.

Invalid credentials return a generic `401`. Workspace and permission failures
inside tools remain concealed as not-found responses where the existing MCP
contract requires that behavior. Dependency failures return server errors and
must not be collapsed into OAuth client errors.

## Expiry, Reauthorization, And Revocation

```mermaid
sequenceDiagram
    autonumber
    participant C as MCP client
    participant M as Proofplane MCP
    participant I as Auth0
    participant X as Post-Login Action
    participant P as Proofplane grant API
    actor U as User

    C->>M: MCP request with expired access token
    M-->>C: 401 Unauthorized
    C->>I: Start Authorization Code with PKCE again

    opt Auth0 session expired
        I-->>U: Universal Login
        U->>I: Authenticate
    end

    I->>X: Run post-login Action
    X->>P: Find active user, client, and resource binding

    alt One active connection
        P-->>X: Active connection, workspace, and scopes
        X->>X: Add existing connection and workspace claims
        I-->>C: Authorization code without Proofplane redirect
        C->>I: Exchange code and PKCE verifier
        I-->>C: New eight-hour access token
        C->>M: Retry MCP request
    else No reusable connection
        alt Silent authorization request
            X-->>I: Deny with interaction_required
            I-->>C: Reconnect visibly
        else Visible authorization request
            X-->>U: Select and approve Proofplane workspace
            U-->>X: Continue Auth0 transaction
            I-->>C: Authorization code
        end
    end
```

When a user revokes a connection, Proofplane first commits local revocation.
That immediately blocks all access tokens through the runtime database check.
The Action refuses to reuse a revoked connection. A silent attempt receives
`interaction_required`; a visible attempt may create a new pending connection
after the user approves a workspace.

Proofplane may also revoke the Auth0 user grant as credential hygiene. Local
revocation remains authoritative because an already-issued access token is
otherwise valid until its eight-hour expiry.

An expired access token cannot be refreshed. Reauthorization always produces
a new authorization code and access token.

## Scope Model

The Auth0 MCP resource server exposes the existing workspace permission
vocabulary:

- `read_evidence_requests`
- `write_evidence_requests`
- `read_evidence_submissions`
- `write_evidence_submissions`
- `read_controls`
- `write_controls`

`offline_access` is not advertised, requested, or accepted in this release.

The Auth0 client grant limits which scopes a client may request. The user
reviews the concrete requested scopes during the workspace step, and Auth0
shows its mandatory third-party application consent when establishing the
user grant. These are distinct: Proofplane binds the request to a workspace;
Auth0 records consent for the client/resource/scope combination. Scope
descriptions must make the two steps consistent rather than contradictory.
Proofplane persists the approved set and intersects it with the signed token on
every request. A tool never escalates a missing scope.

## Persistence And Audit

Proofplane stores:

- `agent_connections`, with at most one active connection per
  user/Auth0-client/resource tuple;
- a single-use consent-continuation nonce or digest;
- structured authorization, use, rejection, and revocation audit events.

An agent connection contains:

- Proofplane connection, user, and workspace IDs;
- Auth0 subject and client ID;
- a client display-name snapshot;
- MCP resource and approved scopes;
- pending expiry, activation, last-use, and revocation timestamps; and
- no raw access token, authorization code, or signing key.

Proofplane does not store OAuth clients or authorization codes locally. Auth0
is authoritative for those protocol objects.

Audit events use identifier-only fields and distinguish human, API-token, and
agent-connection actors. Auth0 tenant logs remain the source for OAuth
issuance events; Proofplane audit records domain approval, runtime use, and
local revocation.

## Public Endpoints And Configuration

Required public roles are:

- MCP resource URL, such as `https://mcp.proofplane.com/mcp`;
- Auth0 issuer URL;
- Auth0 JWKS URL;
- Proofplane workspace-consent URL; and
- Proofplane internal grant-validation URL callable from the Auth0 Action.

The MCP resource URL is both the protected resource identifier and Auth0 API
audience. It must be identical in client requests, Auth0 configuration,
protected-resource metadata, JWT validation, and connection records.

Production endpoints use HTTPS. Local unit and integration tests use local
JWK fixtures and a fake Action caller. End-to-end Auth0 testing uses a
dedicated development tenant and an externally reachable preview environment;
an Auth0 Action cannot call an unexposed loopback service.

Secrets used between the Action and Proofplane are managed as Auth0 Action
secrets and Proofplane runtime secrets. They are rotated independently of
Auth0 JWT signing keys.

The initial MCP deployment may use one replica or ingress stickiness keyed by
`Mcp-Session-Id`. The session ID is transport state, never authorization.

## Website Experience

The website:

- asks which agent the user uses;
- launches the best verified client-specific connection path;
- hosts the workspace-consent route invoked by the Auth0 Action;
- clearly displays client identity, requested permissions, and workspace;
- lists authorized connections and supports local revocation;
- explains that OAuth connections may require reconnection after eight hours;
- provides an explicit Authenticate or Reconnect action when the client cannot
  restart OAuth automatically;
- distinguishes "authorized" from external client connection health;
- offers a first useful SOC 2 prompt; and
- keeps API-token setup as an advanced path.

The consent route is part of the Auth0 transaction. It accepts only a valid
short-lived Action token and state, and it always rechecks membership on
approval. A normal website session alone cannot mint an agent connection.

Proofplane records a pending grant when the workspace is approved and marks it
active only on the first valid Auth0-backed MCP request. The agent harness
still owns whether its transport remains connected and tools are mounted.
`last_used_at` is audit/debug metadata, not a readiness gate.

## Client Distribution

### Claude And Cowork

Proofplane ships as a hosted remote connector. The initial release may use a
custom connector URL. Directory submission follows production Auth0 OAuth,
support, privacy, tool-annotation, and test-account validation.

The release smoke test must prove that Claude follows Protected Resource
Metadata to the Auth0 issuer, completes the Redirect Action workspace step,
and sends an Auth0 access token with the required claims.

### Codex

The `proofplane-soc2` Codex plugin bundles:

- the hosted MCP server declaration;
- focused SOC 2 workflow skills;
- server/tool instructions and safe first prompts; and
- support, privacy, terms, and approval guidance.

It contains no customer credential. Plugin-led OAuth must be validated in the
Codex app before the path is presented as no-token onboarding. If the app
cannot complete Auth0 MCP OAuth, direct Codex setup remains an advanced path
using documented CLI/config behavior.

### Other Clients

Generic clients receive the hosted MCP URL and a capability matrix. OAuth is
offered only to clients with a reviewed Auth0 registration path. Others use
advanced `ppat_` setup.

Open DCR remains deferred until tenant ACL, abuse controls, default API
permissions, cleanup, and client-display behavior are specified and tested.

## Compatibility And Failure Behavior

- Existing REST and MCP `ppat_` callers remain unchanged.
- Unknown issuers, audiences, clients, connections, workspaces, and scopes
  fail closed.
- Missing or malformed Action state cannot create a connection.
- Denied Auth0 consent or abandoned code exchange leaves no active connection;
  the pending grant expires.
- A user without current workspace membership cannot authorize or reuse a
  connection.
- Expired access tokens receive `401` and require a new authorization-code
  flow.
- Revocation and membership removal invalidate MCP access immediately.
- Auth0 or Proofplane dependency failure does not fall back to shared or
  long-lived credentials.
- An unsupported client receives honest fallback guidance.
- Client-directory rejection does not block custom connector or API-token
  setup.

## Testing

- Unit tests validate Auth0 JWT signature, issuer, audience, lifetime, client,
  connection, workspace, and scope claims with local JWK fixtures.
- MCP protocol tests validate `401` challenges and Protected Resource Metadata.
- Action tests cover initial redirect, signed continuation, scope filtering,
  active-connection reuse, silent interaction requirements, denial, and
  unavailable Proofplane services.
- Integration tests cover workspace isolation, live membership removal,
  connection revocation, token expiry, reauthorization, and `ppat_`
  coexistence.
- Browser tests cover workspace approval, denial, expiry, replay, and malformed
  Action transactions.
- Preview smoke tests exercise initial and repeated Auth0 Authorization Code
  with PKCE using MCP Inspector.
- Release smoke tests exercise Claude/Cowork and Codex against a
  production-like environment, including behavior after token expiry.

Proofplane tests do not reproduce Auth0's authorization-code, PKCE, signing,
or protocol-error internals. They test configuration, integration, claim
validation, expiry, and Proofplane policy.

## Delivery Gates And Open Decisions

Implementation must not begin until these tenant capabilities are confirmed in
a development Auth0 tenant:

1. MCP Resource Parameter Compatibility Profile.
2. Third-party client access to the MCP resource server.
3. Post-login Redirect Actions for those third-party clients.
4. An eight-hour access-token configuration without `offline_access`.
5. Active-connection lookup and claim injection on repeated authorization.

The remaining client questions are:

- whether Claude/Cowork supports the selected Auth0 CIMD or manual
  registration model;
- whether Codex plugin installation can initiate and complete the flow without
  CLI/config work; and
- whether Claude/Cowork and Codex automatically restart OAuth after `401` or
  provide a usable reconnect control; and
- whether generic OAuth warrants enabling and securing open DCR.

If a first-class client cannot recover from access-token expiry with an
acceptable visible or automatic authorization flow, it must not be marketed
as a persistent no-token connection. Unattended callers remain on `ppat_`
authentication.

## Distribution Order

1. Validate the Auth0 capability gates with MCP Inspector.
2. Ship MCP discovery, Auth0 JWT validation, and workspace grant binding.
3. Ship connection listing, revocation, and guided website setup.
4. Validate Claude/Cowork custom connector behavior.
5. Validate the Codex plugin preview.
6. Prepare directory or broader marketplace submission.

## Reference Material

- [MCP Authorization specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- [Auth0 MCP authorization flow](https://auth0.com/ai/docs/mcp/intro/why-auth-for-mcp)
- [Auth0 MCP client registration](https://auth0.com/ai/docs/mcp/guides/registering-your-mcp-client-application)
- [Auth0 Resource Parameter Compatibility Profile](https://auth0.com/ai/docs/mcp/guides/resource-param-compatibility-profile)
- [Auth0 Redirect Actions](https://auth0.com/docs/customize/actions/explore-triggers/signup-and-login-triggers/login-trigger/redirect-with-actions)
- [Auth0 Redirect Action limitations](https://auth0.com/docs/customize/actions/explore-triggers/signup-and-login-triggers/login-trigger/redirect-with-actions#restrictions-and-limitations)
- [RFC 9728: Protected Resource Metadata](https://datatracker.ietf.org/doc/html/rfc9728)
- [RFC 8707: Resource Indicators](https://datatracker.ietf.org/doc/html/rfc8707)
- [RFC 9700: OAuth Security Best Current Practice](https://datatracker.ietf.org/doc/html/rfc9700)

## Revisions

- 2026-07-02: Removed `offline_access` and connection-bound refresh metadata
  from the initial release. Auth0 now issues eight-hour access tokens. Repeat
  authorization reuses the one active user/client/resource connection without
  a Proofplane redirect when possible; otherwise the user reconnects visibly.
- 2026-07-02: Replaced the proposed Proofplane OAuth facade and PASETO token
  service with direct Auth0 MCP authorization. Added the Redirect Action
  workspace-grant bridge, connection-bound refresh metadata, runtime claim
  contract, revocation behavior, delivery gates, and end-to-end diagrams.
- 2026-06-29: Initial proposal selected hosted remote MCP, native client
  distribution, and API-token compatibility.
- 2026-06-29: Limited first-class distribution to Claude/Cowork and Codex,
  leaving other clients on generic fallback guidance.
- 2026-06-29: Established one-workspace grants, concrete workspace permission
  scopes, configurable public endpoints, and Proofplane-owned connection
  lifecycle records.
