# 001 - Remote MCP OAuth Foundation

**Status:** Todo · **Depends on:** MCP Server 001 (Done) · **Spec:** [spec.md](../spec.md#remote-mcp-authorization)

**Summary** - Add interactive, standards-compatible authorization to the hosted
Streamable HTTP MCP endpoint so supported clients can open a browser login
instead of requiring a copied `ppat_` token. The website/API owns the OAuth
browser flow and token issuance; the MCP server only exposes discovery,
verifies MCP-scoped credentials, and enforces workspace/scope access.

**Acceptance criteria**

- [ ] Given a compatible unauthenticated MCP client, when it connects, then it
  discovers the website/API authorization service and completes a PKCE browser
  flow without MCP-hosted HTML or static assets.
- [ ] Given local, preview, or production configuration, when discovery
  metadata and tokens are issued, then resource, issuer, and app URLs come from
  validated config rather than hard-coded hostnames.
- [ ] Given an authenticated user, when they grant access, then the resulting
  credential is bound to one selected workspace, one MCP resource, one client,
  and the approved workspace permissions.
- [ ] Given Auth0 authenticates a user, when MCP credentials are issued, then
  Auth0 JWTs are not returned to MCP clients and Proofplane-issued PASETO
  credentials are used instead.
- [ ] Given denied consent, an inaccessible workspace, or an invalid resource,
  when authorization is attempted, then no usable credential is issued.
- [ ] Given an MCP request with a valid MCP-scoped credential, when it reaches
  `/mcp`, then the MCP server verifies the credential and enforces the bound
  workspace and scopes without invoking website UI.
- [ ] Given an existing valid `ppat_` caller, when OAuth support ships, then its
  REST and MCP authentication behavior is unchanged.

**Tasks**

- [ ] Implement the website/API-owned authorization facade for MCP OAuth.
- [ ] Add validated public URL config for MCP resource URL, authorization
  issuer URL, and app base URL.
- [ ] Implement known-client registration and exact redirect-URI policy.
- [ ] Implement protected-resource and authorization-server discovery.
- [ ] Implement browser authorization, workspace consent, PKCE, and resource
  binding outside the MCP server's web surface.
- [ ] Refactor existing PASETO helpers into purpose-specific issuers/verifiers
  without changing attachment download grant behavior.
- [ ] Add a separate MCP OAuth PASETO keyring and implicit assertions for
  access and refresh tokens.
- [ ] Implement 15-minute PASETO access tokens and rotating 30-day idle /
  90-day absolute PASETO refresh tokens.
- [ ] Implement revocation and refresh-token reuse detection.
- [ ] Route OAuth and `ppat_` identities into the existing MCP authorization
  context without exposing credentials to tools.
- [ ] Add protocol and integration tests for success, denial, expiry,
  wrong-resource, refresh reuse, revocation, unknown clients, redirect-URI
  rejection, and workspace isolation.
- [ ] Update local and production configuration documentation.

**Notes**

- 2026-06-29: Spec now fixes the ownership boundary: website/API owns OAuth UI
  and token issuance; MCP owns protocol discovery, token verification, and
  authorization enforcement.
- 2026-06-29: Spec now fixes client registration, scope vocabulary,
  one-workspace grants, opaque credential lifetimes, and revocation behavior.
- 2026-06-29: Spec now requires public endpoint URLs to be environment
  configuration, not hard-coded production hosts.
- 2026-06-29: Dynamic Client Registration was removed from MVP scope. OAuth is
  limited to known clients in this epic.
- 2026-06-29: Spec now uses Proofplane-issued PASETO credentials for MCP OAuth
  and keeps Auth0 JWTs as upstream human identity tokens only.
