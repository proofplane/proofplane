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

MCP is a data-plane interface. Requests authenticate with the same API-key
credential contract as REST and produce the same `ActorContext`. Tool
authorization uses the same workspace permissions as equivalent REST
operations.

The initial transport may accept API-key headers on the HTTP session. Raw keys
must not enter tool arguments, logs, or audit payloads.

## MVP Tools

Read tools:

- `list_evidence_requests`
- `get_evidence_request`
- `list_due_evidence_requests`
- `get_evidence_submission`
- `get_latest_evidence_submission`
- `list_controls`
- `list_evidence_request_control_mappings`
- `find_source_material`
- `preview_auditor_packet`
- `inspect_audit_history`

Write tools:

- `create_evidence_submission`
- `map_evidence_request_to_control`
- `remove_evidence_request_control_mapping`
- `create_or_update_source_material`
- `log_agent_action`

Binary attachment upload/download and packet ZIP transfer remain REST URLs. MCP
returns the relevant attachment content endpoint or packet preview and tells the
agent which REST operation carries bytes. Native approve/reject and derived
control-status tools are not part of the MVP.

## Errors And Equivalence

Validation errors return structured field issues. Authentication,
authorization/not-found, conflict, and dependency failure are distinct MCP
problem codes. Internal error text does not leak database, credential, object
key, or dependency secrets.

Shared operations must produce the same domain result through REST and MCP.
Protocol response shapes may differ.

## Audit

Meaningful MCP reads and every write include client type `mcp`, actor, workspace,
tool name, request/session correlation, and affected object. `log_agent_action`
records a caller-provided action type and rationale under an allowlisted payload
schema.

## Revisions

- 2026-06-11: Removed approval/rejection and binary-transfer tools from the
  legacy story because those behaviors are not in the MVP domain model.
