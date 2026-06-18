# MCP Server Spec

## Goal

Expose the stable compliance service layer through MCP so customer-owned agents
can inspect and update Proofplane without calling REST indirectly.

## Runtime And Transport

The current binary only runs migrations and exits. Implement an MCP server over
streamable HTTP bound to `server.mcp_bind`. The binary owns config, tracing,
migrations, dependency construction, metrics, listener lifecycle, and graceful
shutdown.

Use an MCP SDK rather than hand-rolling JSON-RPC. Keep MCP DTOs and protocol
errors in `src/mcp`; tools call services directly.

## Identity

MCP is a data-plane interface. Requests authenticate with the same user-owned
`v4.public` PASETO bearer token contract as REST and produce the same
`ApiTokenContext`. Tool authorization uses the same workspace permissions as
equivalent REST operations.

The initial transport accepts the bearer token on the HTTP session. Raw tokens
must not enter tool arguments, logs, or audit payloads.

## MVP Tools

Read tools:

- `list_evidence_requests`
- `get_evidence_request`
- `list_due_evidence_requests`
- `get_evidence_submission`
- `get_latest_evidence_submission`
- `create_attachment_download_grant`
- `list_controls`
- `list_evidence_request_control_mappings`
- `find_source_material`
- `preview_auditor_packet`

Write tools:

- `create_evidence_submission`
- `map_evidence_request_to_control`
- `remove_evidence_request_control_mapping`
- `create_or_update_source_material`

`create_attachment_download_grant` is a read-classified tool because it does not
change compliance data. It authorizes the current user API token, verifies the
attachment is finalized, and returns a short-lived Proofplane HTTPS URL for
human inspection. The URL expires after five minutes and may be fetched more
than once before expiry. The URL is a bearer secret; the tool result must tell
the agent not to fetch, summarize, log, or persist it, only present it to the
user. The attachment bytes do not pass through MCP or model context.

Binary attachment upload and packet ZIP transfer remain HTTP operations. Native
approve/reject and derived control-status tools are not part of the MVP.

## Errors And Equivalence

Validation errors return structured field issues. Authentication,
authorization/not-found, conflict, and dependency failure are distinct MCP
problem codes. Internal error text does not leak database, credential, object
key, or dependency secrets.

Shared operations must produce the same domain result through REST and MCP.
Protocol response shapes may differ.

## Audit

Meaningful MCP reads and every write emit a structured audit log with client
type `mcp`, user, API token, workspace, tool name, request/session correlation,
and affected object. There is no audit-history or arbitrary agent-log tool in
the MVP.

## Revisions

- 2026-06-11: Removed approval/rejection and binary-transfer tools from the
  original plan because those behaviors are not in the MVP domain model.
- 2026-06-11: Removed database audit-history and agent-log tools in favor of
  structured application audit logs routed to Cloud Logging.
- 2026-06-11: Added attachment download-grant issuance for human inspection;
  attachment bytes do not pass through the MCP connection.
- 2026-06-17: Replaced the planned actor API-key session with the user-owned
  PASETO bearer-token contract from the PASETO Token Migration epic.
