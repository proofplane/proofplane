# MCP Attachment Management Epic

Let MCP clients delegate evidence attachment upload and download to a human
browser session without moving file bytes through chat, model context, or MCP.

> **Reconciliation — 2026-07-09 (PR #42):** This epic shipped when upload grants
> could be issued via either a `ppat_` API token or an agent connection. `ppat_`
> authentication was removed in PR #42 (see the [Agent Connector
> Onboarding](../agent-connector-onboarding/spec.md) 2026-07-09 decision
> banner), so grants are now always issued via an **agent connection**. The
> `issued_via_api_token_id` column and any "issued via API token" wording in the
> spec/tickets are vestigial; only the agent-connection issuer is populated.

Technical decisions live in [spec.md](./spec.md). Page behavior lives in
[ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Upload Grant Persistence](./tickets/001-upload-grant-persistence.md) | Done | Added durable single-use grant state, upload-grant keyring, and service primitives. |
| 002. [MCP Upload Grant Tool](./tickets/002-mcp-upload-grant-tool.md) | Done | Added the MCP management tool and audit issuance. |
| 003. [Grant Redemption And Upload Session](./tickets/003-grant-redemption-and-upload-session.md) | Done | Redeems one-time URLs into scoped browser upload sessions. |
| 004. [Browser Upload Page](./tickets/004-browser-upload-page.md) | Done | Serves the minimal page, uploads attachments one at a time, and downloads finalized attachments. |

## Sequencing

Build 001 first because single-use behavior needs durable state. Ticket 002 can
then expose the MCP tool that issues URLs. Ticket 003 makes those URLs usable by
turning them into scoped browser sessions. Ticket 004 finishes the human flow on
top of that session.

This epic depends on the existing Evidence Lifecycle Completion upload pipeline
and MCP Server runtime. It deliberately does not depend on serving the Vite UI
app from the Rust API.
