# Sandbox Product Launch Epic

Create the self-serve path from public discovery to a real SOC 2 workspace
connected to the customer's AI agent. The core principle is time to first useful
agent answer, not time spent in browser onboarding.

Technical decisions live in [spec.md](./spec.md); interface behavior lives in
[ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Web Surface Foundation](./tickets/001-web-surface-foundation.md) | Todo | Select and scaffold the browser architecture. |
| 002. [Sandbox Provisioning](./tickets/002-sandbox-provisioning.md) | Todo | Create or resume isolated realistic workspaces. |
| 003. [Agent Credential Setup](./tickets/003-agent-credential-setup.md) | Todo | Expose the safe one-time actor key and MCP configuration flow. |
| 004. [MCP First-Run Experience](./tickets/004-mcp-first-run-experience.md) | Todo | Guide the customer with prompts instead of browser forms. |
| 005. [Marketing And Pricing Pages](./tickets/005-marketing-and-pricing-pages.md) | Todo | Publish answer-first launch content and public pricing. |
| 006. [Discovery Metadata And Funnel](./tickets/006-discovery-metadata-and-funnel.md) | Todo | Add crawler files, structured data, and privacy-safe events. |

## Sequencing

- **001** is foundational.
- **002** can begin with backend APIs while 001 establishes browser conventions.
- **003** depends on 001, 002, Auth Hierarchy API ticket 003, and the MCP
  runtime.
- **004** depends on 003 and the MCP read/write tools used by suggested prompts.
- **005** can proceed in parallel after 001.
- **006** follows stable public routes and first-run events.
