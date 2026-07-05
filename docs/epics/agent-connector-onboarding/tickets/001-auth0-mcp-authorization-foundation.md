# 001 - Auth0 MCP Authorization Foundation

**Status:** Doing · **Depends on:** MCP Server 001 (Done) · **Spec:** [spec.md](../spec.md#architecture-decision)

**Summary** - Make Auth0 the OAuth authorization server for the hosted MCP
resource. Prove the required tenant capabilities, publish MCP discovery
metadata, and validate Auth0-issued MCP access tokens without adding OAuth
protocol or token-issuance endpoints to Proofplane.

**Acceptance criteria**

- [ ] Given the development Auth0 tenant, when the capability spike runs, then
  resource parameters, third-party clients, Authorization Code with PKCE,
  24-hour access tokens, and repeated authorization are proven end to end
  without `offline_access`.
- [ ] Given a compatible unauthenticated MCP client, when it contacts `/mcp`,
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

- [ ] Configure a development Auth0 MCP API, Resource Parameter Compatibility,
  third-party test client, scopes, and a 24-hour access-token lifetime.
- [ ] Prove initial and repeated authorization with MCP Inspector, including
  the behavior after access-token expiry.
- [x] Add validated MCP resource, Auth0 issuer, JWKS, and allowed-client
  configuration.
- [x] Implement `WWW-Authenticate` and Protected Resource Metadata.
- [x] Generalize Auth0 JWT verification for MCP claims and a unified actor
  context while preserving `ppat_` behavior.
- [x] Add local-JWK unit tests and MCP discovery and authorization-boundary
  integration tests.
- [ ] Run Auth0 preview smoke tests with MCP Inspector.
- [x] Document Auth0 tenant provisioning and MCP Inspector validation.

**Notes**

- 2026-07-02: The spec replaces the Proofplane OAuth/PASETO service with
  direct Auth0 authorization using 24-hour access tokens without
  `offline_access`.
- Open DCR remains deferred; first-class clients use reviewed Auth0 CIMD or
  manual third-party registration.
- 2026-07-02: The spec now records the 001/002 fail-closed boundary. See the
  [development tenant runbook](../auth0-development-tenant.md) for provisioning
  and sanitized evidence requirements.
- 2026-07-02: Repository implementation and `make check` are complete. Live
  tenant provisioning, MCP Inspector evidence, and preview smoke validation
  remain before the ticket can move to Done.
- 2026-07-02: The spec now selects Auth0's default `access_token` dialect and
  validates the authorized client through `azp`; RFC 9068 is not required.
- 2026-07-02: The spec now uses Auth0's default 86,400-second access-token
  lifetime. The pasted client-credentials token proves audience and signing
  configuration only; interactive authorization evidence remains required.
- 2026-07-02: The spec correction moves Redirect Actions, continuation,
  namespaced workspace claims, and related secrets entirely to ticket 002.
  Ticket 001 accepts standard Auth0 user access tokens and denies protected
  tools until that workspace authorization exists.
- 2026-07-03: The development runbook now provisions the resource server,
  static third-party Inspector client, user-delegated client grant, and domain
  connection with Auth0 CLI. It follows Auth0's confidential
  `client_secret_post` Inspector contract; Inspector supplies mandatory PKCE.
- 2026-07-04: The spec now records startup validation for the preconstructed
  MCP authentication challenge.
