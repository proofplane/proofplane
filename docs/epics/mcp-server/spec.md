# MCP Server Spec

## Goal

Expose the stable compliance service layer through MCP so customer-owned agents
can inspect and update Proofplane without calling REST indirectly.

## Runtime And Transport

The current binary only runs migrations and exits. Implement an MCP server over
streamable HTTP bound to `server.mcp_bind`. The binary owns config, tracing,
migrations, dependency construction, metrics, listener lifecycle, and graceful
shutdown.

Use `rmcp` 1.7.0 rather than hand-rolling JSON-RPC. The fixed protocol endpoint
is `/mcp`; configured liveness and readiness routes and public `/metrics` share
the listener. Keep MCP DTOs and protocol errors in `src/mcp`; tools call
services directly.

The initial runtime uses rmcp's stateful process-local session manager and
default loopback host protections. It therefore requires a single instance or
sticky routing until distributed session storage and production ingress hosts
are configured. On shutdown, the listener stops accepting requests, drains for
`mcp.shutdown_grace_seconds`, then cancels remaining rmcp sessions and exits.

## Identity

MCP is a data-plane interface. Requests authenticate with the same user-owned
opaque `ppat_` bearer-token contract as REST and produce the same
`ApiTokenContext`. Tool authorization uses the same workspace permissions as
equivalent REST operations.

HTTP MCP authorization is transport-level: the MCP client or agent harness
supplies credentials as HTTP request metadata, outside MCP tool schemas and
outside model-visible tool arguments. A Proofplane tool must never accept,
return, log, audit, or instruct the model to handle a raw `ppat_` token.

Every Streamable HTTP `POST`, `GET`, and `DELETE` request authenticates its
`Authorization: Bearer ppat_...` credential and derives a fresh
`ApiTokenContext`; an `Mcp-Session-Id` identifies protocol/session state only
and never confers authorization. Missing, malformed, unknown, expired, revoked,
and membership-invalid credentials return a generic bearer challenge.
Repository failures return a generic server error.

This is a pre-provisioned API-token model for clients that can be configured
with a Proofplane API token and attach it as the MCP HTTP bearer credential.
OAuth discovery, Protected Resource Metadata, and interactive MCP OAuth login
are not part of this PR's runtime; they are deferred to the separate OAuth
workstream. That workstream may change how MCP clients obtain bearer
credentials, but it should preserve per-request validation and the prohibition
on exposing credentials through tool arguments.

## Core Demo Tools

Read tools:

- `list_evidence_requests`
- `get_evidence_request`
- `list_due_evidence_requests`
- `get_evidence_submission`
- `get_latest_evidence_submission`
- `create_attachment_download_grant`
- `list_frameworks`
- `list_framework_requirements`
- `list_controls`
- `get_control`
- `list_evidence_request_control_mappings`

Write tools:

- `create_evidence_request`
- `create_evidence_submission`
- `create_control`
- `replace_control`
- `map_evidence_request_to_control`
- `remove_evidence_request_control_mapping`

These tools are sufficient for the core MCP demo: an agent can inspect due
requests and latest summarized evidence, create a request or submission, direct
a human to finalized attachments, discover framework requirement IDs, create or
update workspace controls, and inspect or update control mappings.

`list_frameworks` and `list_framework_requirements` expose global standards
reference data. They require a valid API token with `ReadControls` on the
token's workspace, but they do not accept `workspace_id` because framework data
is not workspace-scoped.

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
- 2026-06-22: Fixed Streamable HTTP at `/mcp`, selected rmcp 1.7.0 with local
  stateful sessions and loopback host protection, required authentication on
  every transport request, and defined the bounded shutdown deadline.
- 2026-06-23: Clarified that MCP credentials are client/harness-managed HTTP
  metadata, not tool arguments or model-visible data; Proofplane's initial
  runtime deliberately uses pre-provisioned API tokens while OAuth interop is
  deferred to the separate OAuth workstream.
