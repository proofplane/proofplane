# MCP Attachment Upload Epic

Let MCP clients delegate evidence attachment upload to a human browser session
without moving file bytes through chat, model context, or MCP.

Technical decisions live in [spec.md](./spec.md). Page behavior lives in
[ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Upload Grant Persistence](./tickets/001-upload-grant-persistence.md) | Done | Added durable single-use grant state, upload-grant keyring, and service primitives. |
| 002. [MCP Upload Grant Tool](./tickets/002-mcp-upload-grant-tool.md) | Done | Added `create_attachment_upload_grant` and audit issuance. |
| 003. [Grant Redemption And Upload Session](./tickets/003-grant-redemption-and-upload-session.md) | Done | Redeems one-time URLs into scoped browser upload sessions. |
| 004. [Browser Upload Page](./tickets/004-browser-upload-page.md) | Done | Serves the minimal page, lists existing attachments, and uploads one file at a time. |

## Sequencing

Build 001 first because single-use behavior needs durable state. Ticket 002 can
then expose the MCP tool that issues URLs. Ticket 003 makes those URLs usable by
turning them into scoped browser sessions. Ticket 004 finishes the human flow on
top of that session.

This epic depends on the existing Evidence Lifecycle Completion upload pipeline
and MCP Server runtime. It deliberately does not depend on serving the Vite UI
app from the Rust API.
