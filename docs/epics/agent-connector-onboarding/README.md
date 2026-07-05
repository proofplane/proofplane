# Agent Connector Onboarding Epic

Make Proofplane connectable to Claude/Cowork, Codex, and other agent harnesses
by non-technical compliance users. The core principle is to connect a hosted
account through browser authorization and native client distribution, not ask
the customer to install a server or copy a long-lived API token.
Auth0 owns OAuth protocol and credential issuance. Proofplane owns
workspace-specific consent, durable connection policy, and MCP enforcement.

Full protocol, lifecycle, client, and product decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auth0 MCP Authorization Foundation](./tickets/001-auth0-mcp-authorization-foundation.md) | Done | Established Auth0 discovery, tenant capability, and user-token verification; client-specific reconnect validation remains downstream. |
| 002. [Workspace Grant And Runtime Authorization](./tickets/002-workspace-grant-and-runtime-authorization.md) | Todo | Add the Redirect Action and bind Auth0 tokens to one live Proofplane workspace connection. |
| 003. [Connection Management And Guided UI](./tickets/003-connection-management-and-guided-ui.md) | Todo | Add connection lifecycle UI, revocation, and client-specific setup. |
| 004. [Claude And Cowork Connector](./tickets/004-claude-and-cowork-connector.md) | Todo | Validate the remote connector path and prepare directory distribution. |
| 005. [Codex Compliance Plugin](./tickets/005-codex-compliance-plugin.md) | Todo | Validate plugin-led Codex OAuth before treating Codex as no-token onboarding. |
| 006. [Generic Client Fallback](./tickets/006-generic-client-fallback.md) | Todo | Provide accurate remote-MCP guidance beyond the first-class clients. |

## Sequencing

- **001** proves the required Auth0 tenant features and establishes discovery
  and token verification.
- **002** depends on 001 and adds workspace consent, connection reuse, and live
  Proofplane authorization.
- **003** depends on 002 and adds customer-visible connection lifecycle and
  guided setup.
- **004** and **005** depend on 002 and can proceed in parallel with 003.
- **006** depends on 001 and can proceed in parallel with host-specific
  distribution work.
- Directory review is an external distribution step, not a blocker for custom
  connector, plugin-development, or generic remote-MCP setup.
