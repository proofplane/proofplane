# Auditor Portal Access Epic

Give customers a secure, auditor-only browser portal for reviewing workspace
controls, evidence, and eligible attachments without adding auditors as
workspace members or giving them API tokens.

Full schema, session, OTP, portal, and MCP decisions live in
[spec.md](./spec.md). Browser behavior lives in [ux.md](./ux.md). Tickets below
are lean handoff units that link into those sources of technical depth.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auditor Access Grants](./tickets/001-auditor-access-grants.md) | Done | Grant persistence/service and MCP caller audit emission are in place. |
| 002. [MCP Auditor Link Tools](./tickets/002-mcp-auditor-link-tools.md) | Done | Audited users can create, list, and revoke auditor links through MCP. |
| 003. [Email OTP Verification And Auditor Sessions](./tickets/003-email-otp-verification.md) | Done | OTP verification now creates revocable seven-day auditor sessions. |
| 004. [Auditor Browser Sessions](./tickets/004-auditor-browser-sessions.md) | Absorbed | Folded into 003 to avoid a temporary verification credential. |
| 005. [Portal Read Model](./tickets/005-portal-read-model.md) | Done | Session-authenticated portal data endpoint now returns the read-only graph. |
| 006. [Auditor Attachment Downloads](./tickets/006-auditor-attachment-downloads.md) | Done | Direct session-cookie downloads stream eligible attachments through Proofplane. |
| 007. [Auditor Portal UI](./tickets/007-auditor-portal-ui.md) | Done | Server-rendered browser invite and portal pages are in place. |
| 008. [Worker OTP Email Delivery](./tickets/008-worker-otp-email-delivery.md) | Todo | Move OTP mail to the worker when production mail delivery is added. |

## Sequencing

- **001** is foundational for every later ticket.
- **002** depends on 001 and gives customers a way to issue links through MCP.
- **003** depends on 001, verifies the intended auditor email, and creates the
  auditor session.
- **004** is absorbed by 003.
- **005** depends on 003 because portal reads require an auditor session.
- **006** depends on 005 and the existing finalized attachment download
  behavior.
- **007** depends on 003, 005, and 006 and keeps the first browser UI thin over shipped
  backend flows.
- **008** depends on 003 and should happen with the production mail adapter.

## Deferred Work

Auditor comments, review statuses, auditor requests back to the workspace,
bulk ZIP exports, firm-branded portals, and a separate SPA are deferred until
auditors need workflow features beyond secure read-only review. Worker-backed
OTP mail delivery is deferred until production mail delivery is added.
