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
| 002. [Agent Connection Persistence And Action Contract](./tickets/002-agent-connection-persistence-and-action-contract.md) | Doing | Add the transactional connection lifecycle and authenticated internal Action contract. |
| 003. [Workspace Consent And Auth0 Redirect Action](./tickets/003-workspace-consent-and-auth0-redirect-action.md) | Doing | Consent, continuation, and Action code are implemented; development-tenant smoke verification remains. |
| 004. [MCP Agent Connection Runtime Authorization](./tickets/004-mcp-agent-connection-runtime-authorization.md) | Todo | Enforce connection claims, membership, scopes, and provenance in MCP. |
| 005. [Connection Management And Guided UI](./tickets/005-connection-management-and-guided-ui.md) | Todo | Add connection lifecycle UI, revocation, and client-specific setup. |
| 006. [Claude And Cowork Connector](./tickets/006-claude-and-cowork-connector.md) | Todo | Validate the remote connector path and prepare directory distribution. |
| 007. [Codex Compliance Plugin](./tickets/007-codex-compliance-plugin.md) | Todo | Validate plugin-led Codex OAuth before treating Codex as no-token onboarding. |
| 008. [Generic Client Fallback](./tickets/008-generic-client-fallback.md) | Todo | Provide accurate remote-MCP guidance beyond the first-class clients. |

## Sequencing

- **001** proves the required Auth0 tenant features and establishes discovery
  and token verification.
- **002** depends on 001 and establishes connection persistence and the
  internal Action contract.
- **003** depends on 002 and adds workspace consent and Redirect Action claim
  injection.
- **004** depends on 002 and 003 and adds live MCP authorization.
- **005** depends on 003 and 004 and adds customer-visible connection
  lifecycle and guided setup.
- **006** and **007** depend on 003 and 004 and can proceed in parallel with
  005.
- **008** depends on 001 and can proceed in parallel with host-specific
  distribution work.
- Directory review is an external distribution step, not a blocker for custom
  connector, plugin-development, or generic remote-MCP setup.
