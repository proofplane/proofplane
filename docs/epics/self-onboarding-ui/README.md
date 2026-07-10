# Self-Onboarding UI Epic

> **Reconciliation — 2026-07-09 (PR #42):** This epic is token-centric and now
> partly obsolete. `ppat_` API tokens and the REST data-plane were removed and
> each user has exactly one workspace (see the [Agent Connector
> Onboarding](../agent-connector-onboarding/spec.md) 2026-07-09 decision
> banner). Concretely:
>
> - **Ticket 004 (Scoped Token Creation Flow) is obsolete.** The scoped-token
>   creation UI it delivered was removed with the API-token backend in PR #42
>   (the diff gutted `ui/src/routes/AppRoute.tsx`). When this epic reopens, that
>   flow must be replaced by the OAuth agent-connection flow (Agent Connector
>   Onboarding ticket 005), not restored.
> - **Workspace creation is single-workspace.** The onboarding flow provisions
>   the user's one workspace; there is no multi-workspace selection.
>
> Revalidate the spec, ux, and all "issue a scoped token" language against these
> decisions before reopening any postponed ticket.

Build the minimal Proofplane web UI that turns the current backend APIs into a
self-serve first-run path: explain the operating model, create a workspace,
connect an agent via OAuth, and show a useful workspace state.

Full stack, API, route, and state decisions live in [spec.md](./spec.md).
Interface behavior and visual direction live in [ux.md](./ux.md). Tickets below
are lean handoff units that link into those sources of technical depth.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [UI App Scaffold](./tickets/001-ui-app-scaffold.md) | Done | Create the Vite React app, design tokens, routing shell, and test harness. |
| 002. [Public Explainer And Auth Entry](./tickets/002-public-explainer-and-auth-entry.md) | Done | Explain Proofplane and route users into Auth0 signup/login. |
| 003. [Workspace Onboarding Flow](./tickets/003-workspace-onboarding-flow.md) | Done | Create or resume workspace setup using current workspace APIs. |
| 004. [Scoped Token Creation Flow](./tickets/004-scoped-token-creation-flow.md) | Obsolete | Shipped, then removed with the API-token backend in PR #42. Replace with the OAuth agent-connection flow, not a token flow, when this epic reopens. |
| 005. [MCP Setup Preview](./tickets/005-mcp-setup-preview.md) | Done - Will Do Later | Postponed until MCP is more complete; revalidate spec/UX before reopening. |
| 006. [Workspace Home And Packet Preview](./tickets/006-workspace-home-and-packet-preview.md) | Done - Will Do Later | Postponed until MCP is more complete; revalidate spec/UX before reopening. |
| 007. [Token And Workspace Settings](./tickets/007-token-and-workspace-settings.md) | Done - Will Do Later | Postponed until MCP is more complete; revalidate spec/UX before reopening. |

## Sequencing

- **001** is foundational for every UI ticket.
- **002** can follow 001 and does not require backend changes beyond Auth0
  configuration.
- **003** depends on 001-002 and the completed Auth Hierarchy API workspace
  endpoints.
- **004** is obsolete: the API-token management endpoints it depended on were
  removed in PR #42. It should be reworked into an OAuth agent-connection flow.
- **005** previously depended on 004 for token context; with tokens removed it
  should preview the OAuth connection flow instead.
- **006** depends on 003 and can progressively replace preview/sample data with
  real data as Auditor Portal Access and MCP Server tickets land.
- **007** depends on 003-004 and can proceed in parallel with 005-006 after token
  management works in the UI.
