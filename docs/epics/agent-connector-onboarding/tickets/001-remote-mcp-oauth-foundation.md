# 001 - Remote MCP OAuth Foundation

**Status:** Done · **Depends on:** MCP Server 001 (Done) · **Spec:** [spec.md](../spec.md#remote-mcp-authorization)

**Summary** - Add interactive, standards-compatible authorization to the hosted
Streamable HTTP MCP endpoint so supported clients can open a browser login
instead of requiring a copied `ppat_` token. The website/API owns the OAuth
browser flow and token issuance; the MCP server only exposes discovery,
verifies MCP-scoped credentials, and enforces workspace/scope access.

**Acceptance criteria**

- [x] Given a compatible unauthenticated MCP client, when it connects, then it
  discovers the website/API authorization service and completes a PKCE browser
  flow without MCP-hosted HTML or static assets.
- [x] Given local, preview, or production configuration, when discovery
  metadata and tokens are issued, then resource, issuer, and app URLs come from
  validated config rather than hard-coded hostnames.
- [x] Given an authenticated user, when they grant access, then the resulting
  credential is bound to one selected workspace, one MCP resource, one client,
  and the approved workspace permissions.
- [x] Given Auth0 authenticates a user, when MCP credentials are issued, then
  Auth0 JWTs are not returned to MCP clients and Proofplane-issued PASETO
  credentials are used instead.
- [x] Given denied consent, an inaccessible workspace, or an invalid resource,
  when authorization is attempted, then no usable credential is issued.
- [x] Given an MCP request with a valid MCP-scoped credential, when it reaches
  `/mcp`, then the MCP server verifies the credential and enforces the bound
  workspace and scopes without invoking website UI.
- [x] Given an existing valid `ppat_` caller, when OAuth support ships, then its
  REST and MCP authentication behavior is unchanged.

**Tasks**

- [x] Implement the website/API-owned authorization facade for MCP OAuth.
- [x] Add validated public URL config for MCP resource URL, authorization
  issuer URL, and app base URL.
- [x] Implement known-client registration and exact redirect-URI policy.
- [x] Implement protected-resource and authorization-server discovery.
- [x] Implement browser authorization, workspace consent, PKCE, and resource
  binding outside the MCP server's web surface.
- [x] Refactor existing PASETO helpers into purpose-specific issuers/verifiers
  without changing attachment download grant behavior.
- [x] Add a separate MCP OAuth PASETO keyring and implicit assertions for
  access and refresh tokens.
- [x] Implement 15-minute PASETO access tokens and rotating 30-day idle /
  90-day absolute PASETO refresh tokens.
- [x] Implement revocation and refresh-token reuse detection.
- [x] Route OAuth and `ppat_` identities into the existing MCP authorization
  context without exposing credentials to tools.
- [x] Add protocol and integration tests for success, denial, expiry,
  wrong-resource, refresh reuse, revocation, unknown clients, redirect-URI
  rejection, and workspace isolation.
- [x] Update local and production configuration documentation.

**Notes**

- 2026-06-30: Spec revision records the `v4.public` OAuth keyring and
  private/public runtime separation.
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
