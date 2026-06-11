# Sandbox Product Launch Epic

Create the self-serve path from public discovery to a real SOC 2 workspace and
auditor packet preview. The core principle is time to first artifact, not time
spent in onboarding.

Technical decisions live in [spec.md](./spec.md); interface behavior lives in
[ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Web Surface Foundation](./tickets/001-web-surface-foundation.md) | Todo | Select and scaffold the browser architecture. |
| 002. [Sandbox Provisioning](./tickets/002-sandbox-provisioning.md) | Todo | Create or resume isolated realistic workspaces. |
| 003. [First-Run Product Flow](./tickets/003-first-run-product-flow.md) | Todo | Create/edit control, request, mapping, and preview. |
| 004. [Agent Credential Setup](./tickets/004-agent-credential-setup.md) | Todo | Expose the safe one-time actor key flow. |
| 005. [Marketing And Pricing Pages](./tickets/005-marketing-and-pricing-pages.md) | Todo | Publish answer-first launch content and public pricing. |
| 006. [Discovery Metadata And Funnel](./tickets/006-discovery-metadata-and-funnel.md) | Todo | Add crawler files, structured data, and privacy-safe events. |

## Sequencing

- **001** is foundational.
- **002** can begin with backend APIs while 001 establishes browser conventions.
- **003** depends on 001, 002, and auditor packet preview.
- **004** depends on Auth Hierarchy API ticket 003.
- **005** can proceed in parallel after 001.
- **006** follows stable public routes and first-run events.
