# Agent Connector Onboarding Epic

Make Proofplane connectable to Claude/Cowork, Codex, and other agent harnesses
by non-technical compliance users. The core principle is to connect a hosted
account through browser authorization and native client distribution, not ask
the customer to install a server or copy a long-lived API token.
The website/API owns OAuth UI and token issuance; the MCP server remains a
protocol endpoint that verifies MCP-scoped bearer credentials.

Full protocol, lifecycle, client, and product decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Remote MCP OAuth Foundation](./tickets/001-remote-mcp-oauth-foundation.md) | Done | Website/API OAuth with MCP-side discovery and public-key token verification. |
| 002. [Connection Lifecycle And Audit](./tickets/002-connection-lifecycle-and-audit.md) | Todo | List and revoke client connections with attributable lifecycle events. |
| 003. [Guided Agent Connection UI](./tickets/003-guided-agent-connection-ui.md) | Todo | Replace token-centric setup with client selection, authorization progress, and verification. |
| 004. [Claude And Cowork Connector](./tickets/004-claude-and-cowork-connector.md) | Todo | Validate the remote connector path and prepare directory distribution. |
| 005. [Codex Compliance Plugin](./tickets/005-codex-compliance-plugin.md) | Todo | Validate plugin-led Codex OAuth before treating Codex as no-token onboarding. |
| 006. [Generic Client Fallback](./tickets/006-generic-client-fallback.md) | Todo | Provide accurate remote-MCP guidance beyond the first-class clients. |

## Sequencing

- **001** is the foundation for every no-token connection path.
- **002** depends on 001 and establishes the customer-visible and auditable
  connection lifecycle.
- **003** depends on 001-002 so the UI reports verified state and can manage
  completed connections.
- **004** and **005** depend on 001 and can proceed in parallel while 002-003
  are built.
- **006** depends on 001 and can proceed in parallel with host-specific
  distribution work.
- Directory review is an external distribution step, not a blocker for custom
  connector, plugin-development, or generic remote-MCP setup.
