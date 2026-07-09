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
| 002. [Agent Connection Persistence](./tickets/002-agent-connection-persistence.md) | Doing | Add the transactional connection lifecycle for OAuth-backed agent grants. |
| 003. [Proofplane OAuth Workspace Consent](./tickets/003-proofplane-oauth-workspace-consent.md) | Doing | Add Proofplane-hosted OAuth consent after upstream Auth0 login. |
| 004. [MCP Agent Connection Runtime Authorization](./tickets/004-mcp-agent-connection-runtime-authorization.md) | Done | Proofplane PASETO MCP tokens activate authorized rows, enforce live membership/scopes, and audit/persist agent provenance for MCP tool use. |
| 005. [Connection Management And Guided UI](./tickets/005-connection-management-and-guided-ui.md) | Todo | Add connection lifecycle UI, revocation, and client-specific setup. |
| 006. [Claude And Cowork Connector](./tickets/006-claude-and-cowork-connector.md) | Todo | DCR removes Proofplane-side static client setup where supported; still validate Claude/Cowork host behavior, expiry, and directory requirements. |
| 007. [Codex Direct MCP Integration](./tickets/007-codex-direct-mcp-integration.md) | Done | Codex connects through direct remote MCP setup, Proofplane DCR, and Proofplane workspace consent; no plugin is required. |
| 008. [Generic Client Fallback](./tickets/008-generic-client-fallback.md) | Todo | Split supported OAuth/DCR clients from unsupported clients that still need advanced API-token setup. |

## Sequencing

- **001** is superseded where it made Auth0 the MCP authorization server.
  Auth0 tenant work remains only for the upstream human login application.
- **002** depends on 001 and establishes connection persistence.
- **003** depends on 002 and adds Proofplane-hosted OAuth consent after
  upstream Auth0 login.
- **004** depends on 002 and 003 and adds live MCP authorization.
- **005** depends on 003 and 004 and adds customer-visible connection
  lifecycle and guided setup.
- **006** depends on 003 and 004 for full tool-use validation. Its remaining
  work is host validation and distribution review.
- **007** is complete through direct Codex MCP login with Proofplane DCR. A Codex
  plugin is no longer part of this epic.
- **008** depends on the Proofplane OAuth facade and should describe supported
  OAuth/DCR clients
  and unsupported clients that remain on advanced API-token setup.
- Directory review is an external distribution step, not a blocker for custom
  connector or generic remote-MCP setup.
