# Agent Connector Onboarding Epic

Make Proofplane connectable to Claude/Cowork, Codex, and other agent harnesses
by non-technical compliance users. The core principle is to connect a hosted
account through browser authorization and native client distribution, not ask
the customer to install a server or copy a long-lived API token.
Proofplane owns MCP OAuth discovery, local Dynamic Client Registration,
Authorization Code with PKCE, account-level consent with an internal
single-workspace binding, PASETO access-token issuance, durable connection
policy, and MCP enforcement. Auth0 remains the upstream human login provider
only.

Full protocol, lifecycle, client, and product decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it. Customer-visible consent, guided setup, and
connection states are captured in [ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auth0 MCP Authorization Foundation](./tickets/001-auth0-mcp-authorization-foundation.md) | Done | Historical delivery evidence retained; its Auth0-owned MCP OAuth architecture was later superseded by the Proofplane OAuth facade. |
| 002. [Agent Connection Persistence](./tickets/002-agent-connection-persistence.md) | Done | Transactional connection lifecycle for OAuth-backed agent grants shipped in PR #42. |
| 003. [Proofplane OAuth Workspace Consent](./tickets/003-proofplane-oauth-workspace-consent.md) | Done | Proofplane-hosted consent after Auth0 login shipped in PR #42; the server binds the user's single workspace internally. |
| 004. [MCP Agent Connection Runtime Authorization](./tickets/004-mcp-agent-connection-runtime-authorization.md) | Done | Proofplane PASETO MCP tokens activate authorized rows, enforce live membership/scopes, and audit/persist agent provenance for MCP tool use. |
| 005. [Connection Management And Guided UI](./tickets/005-connection-management-and-guided-ui.md) | Done | Account-level consent, user-scoped lifecycle APIs, authoritative revocation, and verified desktop setup shipped. |
| 006. [Claude And Cowork Connector](./tickets/006-claude-and-cowork-connector.md) | Done | `mcp.allowed_hosts` fix + ngrok preview shipped; Codex and Cowork work end to end. Exact post-expiry UX is a deferred refresh-token follow-up. |
| 007. [Codex Direct MCP Integration](./tickets/007-codex-direct-mcp-integration.md) | Done | Codex connects through direct remote MCP setup, Proofplane DCR, and account-level Proofplane consent; no plugin is required. |
| 008. [Generic Client Compatibility](./tickets/008-generic-client-compatibility.md) | Todo | Define and verify guidance for OAuth/DCR-capable clients; no credential fallback exists for unsupported clients. |

## Sequencing

- **001–007 are Done.** The Proofplane OAuth facade, connection persistence,
  account-level consent with an internal single-workspace binding, runtime
  authorization, guided connection management, hosted-client support, and the
  direct Codex path have shipped.
- **001** retains its completed foundation evidence, but its Auth0-owned MCP
  authorization-server architecture was superseded. Auth0 now provides only
  upstream human login.
- **005** delivered customer-visible connection lifecycle and guided setup on
  top of tickets 003 and 004. OAuth is the only setup path.
- **006** validated hosted-client tool use through Cowork after adding the MCP
  host allowlist. Exact post-expiry UX and directory distribution remain
  follow-up work.
- **008 is the only remaining ticket.** It depends on tickets 003 and 004 and
  will define and verify compatibility guidance for generic OAuth/DCR-capable
  clients. There is no bearer-token fallback for unsupported clients.
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
