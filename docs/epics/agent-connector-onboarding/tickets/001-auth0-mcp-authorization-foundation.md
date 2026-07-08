# 001 - Auth0 MCP Authorization Foundation

**Status:** Superseded · **Depends on:** MCP Server 001 (Done) · **Spec:** [spec.md](../spec.md#architecture-decision)

**Summary** - Superseded foundation work that made Auth0 the MCP OAuth
authorization server. The current design makes Proofplane the MCP OAuth
authorization server facade and keeps Auth0 only for upstream human login.

**Acceptance criteria**

- [x] Given the development Auth0 tenant, when the capability spike runs, then
  Resource Parameter Compatibility, a third-party Inspector client,
  Authorization Code with PKCE, configured workspace scopes, and a 24-hour
  access-token lifetime without `offline_access` are demonstrated through the
  browser authorization return.
- [x] Given a compatible unauthenticated MCP client, when it contacts `/mcp`,
  then Protected Resource Metadata directs it to Auth0 and Auth0 publishes the
  authorization endpoints and PKCE capabilities.
- [x] Given a valid Auth0 access token for the canonical MCP resource, when it
  reaches `/mcp`, then Proofplane verifies its signature, issuer, audience,
  lifetime, user subject, authorized client, and known workspace scopes.
- [x] Given a wrong algorithm, signature, issuer, audience, client, lifetime,
  machine identity, unsupported scope, or unavailable JWKS, when
  authentication runs, then access fails closed with the correct client or
  server error class.
- [x] Given a verified Auth0 user without a ticket 002 workspace grant, when it
  initializes MCP or lists tools, then those protocol operations succeed and
  every protected tool remains denied.
- [x] Given an existing valid `ppat_` caller, when OAuth support ships, then its
  REST and MCP authentication behavior is unchanged.

**Tasks**

- [x] Configure a development Auth0 MCP API, Resource Parameter Compatibility,
  third-party test client, scopes, and a 24-hour access-token lifetime.
- [x] Exercise initial browser authorization with MCP Inspector through the
  return from Auth0.
- [x] Add validated MCP resource, Auth0 issuer, JWKS, and required
  client-identity validation.
- [x] Implement `WWW-Authenticate` and Protected Resource Metadata.
- [x] Generalize Auth0 JWT verification for MCP claims and a unified actor
  context while preserving `ppat_` behavior.
- [x] Add local-JWK unit tests and MCP discovery and authorization-boundary
  integration tests.
- [x] Record the Auth0 tenant contract and MCP Inspector validation outcome.

**Notes**

- 2026-07-02: The spec replaces the Proofplane OAuth/PASETO service with
  direct Auth0 authorization using 24-hour access tokens without
  `offline_access`.
- 2026-07-07: Superseded. Proofplane now owns MCP OAuth discovery, DCR,
  Authorization Code with PKCE, one-use code exchange, and 24-hour PASETO MCP
  access tokens. Auth0 remains the upstream human login provider.
- Development DCR is now the default Codex path; production DCR remains
  deferred until tenant ACL, abuse controls, monitoring, cleanup, and default
  third-party API permissions are specified.
- 2026-07-02: The spec records the 001/002 fail-closed boundary and development
  tenant contract.
- 2026-07-02: Repository implementation and `make check` are complete.
- 2026-07-02: The spec now selects Auth0's default `access_token` dialect and
  validates the authorized client through `azp`; RFC 9068 is not required.
- 2026-07-02: The spec uses Auth0's default 86,400-second access-token lifetime.
- 2026-07-02: The spec correction moves Redirect Actions, continuation,
  namespaced workspace claims, and related secrets downstream of ticket 001
  (now split across tickets 002-004).
  Ticket 001 accepts standard Auth0 user access tokens and denies protected
  tools until that workspace authorization exists.
- 2026-07-03: The development tenant uses a static third-party Inspector
  client, user-delegated client grant, domain connection, and confidential
  `client_secret_post` contract; Inspector supplies mandatory PKCE.
- 2026-07-07: Codex `0.142.5` successfully probed Protected Resource
  Metadata, Auth0 authorization-server metadata, and the DCR registration
  endpoint. Proofplane no longer requires local MCP client allowlisting.
- 2026-07-04: The spec now records startup validation for the preconstructed
  MCP authentication challenge.
- 2026-07-05: MCP Inspector reached Auth0 through Proofplane discovery,
  completed Google authentication, and returned to Inspector. Inspector 0.22.0
  then bypassed its configured proxy during callback continuation; this is an
  external harness limitation, not a foundation blocker. Reauthorization after
  token expiry remains a client-specific validation item in downstream
  tickets. See the 2026-07-05 spec revision.
