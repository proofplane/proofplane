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
opaque `ppat_` bearer-token contract as REST and produce the same
`ApiTokenContext`. Tool authorization uses the same workspace permissions as
equivalent REST operations.

The initial transport accepts the bearer token on the HTTP session. Raw tokens
must not enter tool arguments, logs, or audit payloads.

## Core Demo Tools

Read tools:

- `list_evidence_requests`
- `get_evidence_request`
- `list_due_evidence_requests`
- `get_evidence_submission`
- `get_latest_evidence_submission`
- `create_attachment_download_grant`
- `list_controls`
- `list_evidence_request_control_mappings`

Write tools:

- `create_evidence_submission`
- `map_evidence_request_to_control`
- `remove_evidence_request_control_mapping`

These tools are sufficient for the core MCP demo: an agent can inspect due
requests and latest summarized evidence, create a submission, direct a human to
finalized attachments, and inspect or update control mappings.

## Auditor Packet Tools

Read tools:

- `preview_auditor_packet`
- `get_auditor_packet_export`
- `create_auditor_packet_download_grant`

Write tools:

- `request_auditor_packet_export`

The packet tools are an additive workflow built after the core demo. They
consume the Trusted Compliance Reads packet-read model and are not prerequisites
for demonstrating the core evidence lifecycle through MCP.

## Context-Efficient Results

MCP tools return the smallest result needed for the next decision and do not
mirror verbose REST response shapes mechanically:

- `create_evidence_submission` accepts optional summary and description fields
  but returns only the created submission ID and compact upload-next-step data;
- `get_latest_evidence_submission` returns the summary but never the
  description;
- `get_evidence_submission` is the deliberate direct-detail operation and may
  return both summary and description;
- `preview_auditor_packet` returns readiness metadata and gaps without either
  free-text field;
- `request_auditor_packet_export` returns only export ID and status;
- `get_auditor_packet_export` returns compact lifecycle/result metadata and
  bounded polling guidance;
- `create_auditor_packet_download_grant` returns a browser URL only for a ready
  export and never returns or fetches ZIP bytes;
- tool results do not duplicate structured fields in explanatory prose.

Attachment and packet download-grant tools are read-classified because they do
not change compliance data. Each authorizes the current user API token, verifies
the object is eligible, and returns a short-lived Proofplane HTTPS URL for human
inspection. URLs expire after five minutes and may be fetched more than once
before expiry. A URL is a bearer secret; the tool result must tell the agent not
to fetch, summarize, log, or persist it, only present it to the user. Attachment
and ZIP bytes do not pass through MCP or model context.

Binary attachment upload and packet ZIP download remain HTTP operations. MCP
may request and poll an asynchronous packet export and create its human download
grant, but never transports the ZIP. Native approve/reject and derived
control-status tools are not part of the MVP.

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
  structured application audit logs. Production routing is deferred to
  deployment planning.
- 2026-06-11: Added attachment download-grant issuance for human inspection;
  attachment bytes do not pass through the MCP connection.
- 2026-06-17: Replaced the planned actor API-key session with the then-planned
  user-owned PASETO bearer-token contract from the PASETO Token Migration epic.
- 2026-06-19: Followed the API-token epic pivot from `v4.public` PASETO to the
  compact opaque `ppat_` bearer-token contract.
- 2026-06-20: Removed the deferred source-material tools. Added bounded
  submission context with selective MCP disclosure: summaries only on focused
  submission reads and descriptions only on direct-by-ID retrieval.
- 2026-06-20: Added asynchronous packet export request/status/grant tools. The
  worker persists the ZIP and the agent presents a browser grant URL without
  mediating packet bytes.
- 2026-06-22: Split core evidence tools from additive auditor-packet tools so
  packet freshness and export work no longer block the core MCP demo.
