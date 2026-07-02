# 001 - Auth0 MCP Authorization Foundation

**Status:** Todo · **Depends on:** MCP Server 001 (Done) · **Spec:** [spec.md](../spec.md#architecture-decision)

**Summary** - Make Auth0 the OAuth authorization server for the hosted MCP
resource. Prove the required tenant capabilities, publish MCP discovery
metadata, and validate Auth0-issued MCP access tokens without adding OAuth
protocol or token-issuance endpoints to Proofplane.

**Acceptance criteria**

- [ ] Given the development Auth0 tenant, when the capability spike runs, then
  resource parameters, third-party clients, Redirect Actions, eight-hour
  access tokens, and repeated authorization are proven end to end without
  `offline_access`.
- [ ] Given a compatible unauthenticated MCP client, when it contacts `/mcp`,
  then Protected Resource Metadata directs it to Auth0 and Auth0 publishes the
  authorization endpoints and PKCE capabilities.
- [ ] Given a valid Auth0 access token for the canonical MCP resource, when it
  reaches `/mcp`, then Proofplane verifies its signature, issuer, audience,
  lifetime, client, scopes, and required Proofplane claims.
- [ ] Given a wrong issuer, audience, client, expired token, missing custom
  claim, or unavailable JWKS, when authentication runs, then access fails
  closed with the correct client or server error class.
- [ ] Given the Auth0-backed flow, when repository routes are inspected, then
  Proofplane exposes no OAuth authorize, token, refresh, or revoke endpoint.
- [ ] Given an existing valid `ppat_` caller, when OAuth support ships, then its
  REST and MCP authentication behavior is unchanged.

**Tasks**

- [ ] Configure a development Auth0 MCP API, Resource Parameter Compatibility,
  third-party test client, scopes, and an eight-hour access-token lifetime.
- [ ] Prove initial and repeated authorization with MCP Inspector, including
  the behavior after access-token expiry.
- [ ] Add validated MCP resource, Auth0 issuer, JWKS, and claim-namespace
  configuration.
- [ ] Implement `WWW-Authenticate` and Protected Resource Metadata.
- [ ] Generalize Auth0 JWT verification for MCP claims and a unified actor
  context while preserving `ppat_` behavior.
- [ ] Add local-JWK unit tests, MCP discovery tests, and Auth0 preview smoke
  tests.
- [ ] Document Auth0 tenant provisioning, secrets, and environment setup.

**Notes**

- 2026-07-02: The spec replaces the Proofplane OAuth/PASETO service with
  direct Auth0 authorization using eight-hour access tokens without
  `offline_access`.
- Open DCR remains deferred; first-class clients use reviewed Auth0 CIMD or
  manual third-party registration.
