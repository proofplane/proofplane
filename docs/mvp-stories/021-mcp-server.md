# 021 - MCP Server

## Goal

Expose the core compliance backend through MCP as a first-class interface.

## Design

Create the `proofplane-mcp` binary. It should use the same domain services as the REST API and keep MCP DTOs separate from API DTOs.

Initial tools:

- `list_evidence_requests`
- `get_evidence_request`
- `list_due_evidence_requests`
- `submit_evidence_for_requirement`
- `get_submission_status`
- `create_attachment_download_grant`
- `get_latest_approved_submission`
- `approve_evidence_submission`
- `reject_evidence_submission`
- `map_requirement_to_control`
- `remove_requirement_control_mapping`
- `list_requirement_control_mappings`
- `get_control_status`
- `get_control_evidence_gaps`
- `find_approved_answer_material`
- `create_or_update_approved_answer_material`

## Acceptance Criteria

- MCP binary starts from YAML config and initializes observability.
- MCP tools call service layer directly, not REST endpoints.
- Tool input validation uses the applicative validation framework.
- Tool errors are structured for agent consumption.
- Authentication or client identity is captured as actor context.
- MCP emits structured `type = "audit_log"` records for meaningful reads and
  writes.

## Tests

- Unit tests cover tool DTO validation and service mapping.
- Integration tests start MCP server against testcontainers dependencies.
- Integration tests call representative read and write tools.
- Tests verify MCP and API produce equivalent domain outcomes for shared operations.
- Audit tests verify MCP client identity is recorded.

## QA Guide

1. Start dependencies and seed data.
2. Start MCP server.
3. Call `list_due_evidence_requests`.
4. Submit evidence through MCP and approve it.
5. Query control status through both MCP and API and compare results.
