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
| 001. [Auditor Access Grants](./tickets/001-auditor-access-grants.md) | Doing | Grant persistence/service is in place; caller-surface audit emission remains. |
| 002. [MCP Auditor Link Tools](./tickets/002-mcp-auditor-link-tools.md) | Todo | Let audited users create, list, and revoke auditor links through MCP. |
| 003. [Email OTP Verification](./tickets/003-email-otp-verification.md) | Todo | Prove the browser user controls the intended auditor email. |
| 004. [Auditor Browser Sessions](./tickets/004-auditor-browser-sessions.md) | Todo | Create revocable seven-day browser sessions after OTP verification. |
| 005. [Portal Read Model](./tickets/005-portal-read-model.md) | Todo | Assemble the workspace-wide read-only auditor data graph. |
| 006. [Auditor Attachment Downloads](./tickets/006-auditor-attachment-downloads.md) | Todo | Stream eligible evidence attachments to verified auditor sessions. |
| 007. [Auditor Portal UI](./tickets/007-auditor-portal-ui.md) | Todo | Add the minimal server-rendered browser portal. |

## Sequencing

- **001** is foundational for every later ticket.
- **002** depends on 001 and gives customers a way to issue links through MCP.
- **003** depends on 001 and verifies the intended auditor email before access.
- **004** depends on 003 and avoids repeated OTP prompts during real reviews.
- **005** depends on 004 because portal reads require an auditor session.
- **006** depends on 005 and the existing finalized attachment download
  behavior.
- **007** depends on 003-006 and keeps the first browser UI thin over shipped
  backend flows.

## Deferred Work

Auditor comments, review statuses, auditor requests back to the workspace,
bulk ZIP exports, firm-branded portals, and a separate SPA are deferred until
auditors need workflow features beyond secure read-only review.
