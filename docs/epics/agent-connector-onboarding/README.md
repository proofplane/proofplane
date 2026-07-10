# Agent Connector Onboarding Epic

Make Proofplane connectable to Claude/Cowork, Codex, and other agent harnesses
by non-technical compliance users. The core principle is to connect a hosted
account through browser authorization and native client distribution, not ask
the customer to install a server or copy a long-lived API token.
Proofplane owns MCP OAuth discovery, local Dynamic Client Registration,
Authorization Code with PKCE, workspace consent, PASETO access-token issuance,
durable connection policy, and MCP enforcement. Auth0 remains the upstream
human login provider only.

Full protocol, lifecycle, client, and product decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auth0 MCP Authorization Foundation](./tickets/001-auth0-mcp-authorization-foundation.md) | Superseded | Auth0-owned MCP OAuth discovery and token issuance are superseded by the Proofplane OAuth facade; Auth0 remains upstream human login. |
| 002. [Agent Connection Persistence](./tickets/002-agent-connection-persistence.md) | Done | Transactional connection lifecycle for OAuth-backed agent grants shipped in PR #42. |
| 003. [Proofplane OAuth Workspace Consent](./tickets/003-proofplane-oauth-workspace-consent.md) | Done | Proofplane-hosted consent after Auth0 login shipped in PR #42; consent binds the user's single workspace as a fixed approval. |
| 004. [MCP Agent Connection Runtime Authorization](./tickets/004-mcp-agent-connection-runtime-authorization.md) | Done | Proofplane PASETO MCP tokens activate authorized rows, enforce live membership/scopes, and audit/persist agent provenance for MCP tool use. |
| 005. [Connection Management And Guided UI](./tickets/005-connection-management-and-guided-ui.md) | Todo | Add connection lifecycle UI, revocation, and client-specific setup. Drop the removed API-token "advanced path" from scope. |
| 006. [Claude And Cowork Connector](./tickets/006-claude-and-cowork-connector.md) | Doing | `mcp.allowed_hosts` fix + ngrok preview shipped; live Claude/Cowork validation and expiry behavior pending. OAuth facade already client-generic. |
| 007. [Codex Direct MCP Integration](./tickets/007-codex-direct-mcp-integration.md) | Done | Codex connects through direct remote MCP setup, Proofplane DCR, and Proofplane workspace consent; no plugin is required. |
| 008. [Generic Client Fallback](./tickets/008-generic-client-fallback.md) | Todo | Reframe: OAuth/DCR-capable clients are supported; there is no `ppat_` fallback for the rest (API tokens removed in PR #42). |

## Sequencing

- **001–004 and 007 are Done.** The Proofplane OAuth facade, connection
  persistence, workspace consent, runtime authorization, and the direct Codex
  path all shipped in PR #42. See the 2026-07-09 decision banner in
  [spec.md](./spec.md) for the `ppat_` removal, REST data-plane removal, and
  one-workspace decisions that landed with it.
- **001** is superseded where it made Auth0 the MCP authorization server.
  Auth0 tenant work remains only for the upstream human login application.
- **005** depends on 003 and 004 and adds customer-visible connection
  lifecycle and guided setup. It must drop the removed API-token "advanced
  path" from its scope.
- **006** depends on 003 and 004 for full tool-use validation. Its remaining
  work is host validation and distribution review.
- **008** depends on the Proofplane OAuth facade and now only describes which
  clients are OAuth/DCR-capable. With `ppat_` removed there is no advanced
  bearer-token fallback for the rest.
- Directory review is an external distribution step, not a blocker for custom
  connector or generic remote-MCP setup.

## Deferred follow-ups

Captured while scoping ticket 006 (2026-07-10), not yet ticketed:

- **Refresh-token support.** v1 issues no refresh token, so Claude re-consents
  every ~24h. Advertising `offline_access` and issuing + rotating refresh
  tokens (with `invalid_grant` on invalidation) would give Claude/Cowork — and
  Codex — a persistent connection. Requires a spec revision (the spec currently
  commits to no refresh tokens in v1).
- **Directory submission.** At directory scale Claude discourages DCR in favor
  of CIMD (`client_id_metadata_document_supported`) or Anthropic-held
  credentials (`oauth_anthropic_creds`), plus privacy/support/test-account and
  tool-annotation metadata. Its own future ticket.
- **Durable preview/staging environment.** Ticket 006 validates via a local
  stack + ngrok; a hosted staging deployment would make Claude testing and
  directory review repeatable.
