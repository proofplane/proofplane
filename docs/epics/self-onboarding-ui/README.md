# Self-Onboarding UI Epic

Build the minimal Proofplane web UI that turns the product promise into a
self-serve first-run path: explain the operating model, create a workspace,
issue a scoped token, preview MCP setup, and land users in a useful SOC 2
sandbox instead of an empty dashboard.

Full stack, API, route, and state decisions live in [spec.md](./spec.md).
Interface behavior and visual direction live in [ux.md](./ux.md). Tickets below
are lean handoff units that link into those sources of technical depth.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [UI App Scaffold](./tickets/001-ui-app-scaffold.md) | Todo | Create the Vite React app, design tokens, routing shell, and test harness. |
| 002. [Public Explainer And Auth Entry](./tickets/002-public-explainer-and-auth-entry.md) | Todo | Explain Proofplane and route users into Auth0 signup/login. |
| 003. [Workspace Onboarding Flow](./tickets/003-workspace-onboarding-flow.md) | Todo | Create or resume workspace setup with sandbox default. |
| 004. [Scoped Token Creation Flow](./tickets/004-scoped-token-creation-flow.md) | Todo | Create a one-time token with job-based permission presets. |
| 005. [MCP Setup Preview](./tickets/005-mcp-setup-preview.md) | Todo | Show honest install/config guidance and suggested prompts. |
| 006. [Sandbox Home And Packet Preview](./tickets/006-sandbox-home-and-packet-preview.md) | Todo | Show controls, evidence status, readiness, and packet preview/unavailable states. |
| 007. [Token And Workspace Settings](./tickets/007-token-and-workspace-settings.md) | Todo | List/revoke tokens and expose workspace identity/settings. |

## Sequencing

- **001** is foundational for every UI ticket.
- **002** can follow 001 and does not require backend changes beyond Auth0
  configuration.
- **003** depends on 001-002 and the completed Auth Hierarchy API workspace
  endpoints.
- **004** depends on 003 and the completed API-token management endpoints.
- **005** depends on 004 for token context, but it can ship with preview labels
  before the MCP Server epic is done.
- **006** depends on 003 and can progressively replace preview/sample data with
  real data as Trusted Compliance Reads and MCP Server tickets land.
- **007** depends on 003-004 and can proceed in parallel with 005-006 after token
  management works in the UI.
