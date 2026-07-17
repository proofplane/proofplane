# 002 - MCP Policy Catalog Tools

**Status:** Done · **Depends on:** [001](./001-policy-domain-and-persistence.md) · **Spec:** [spec.md](../spec.md#mcp-contract)

**Summary** - Let authorized MCP connections list, inspect, create, edit,
archive, attach, and detach policies without adding a second compliance
data-plane.

**Acceptance criteria**

- [x] Given `read_controls`, when an active policy is listed or fetched, then
  compact, deterministic policy data is returned without object storage or
  archived-history details.
- [x] Given `write_controls`, when valid create, update, archive, attach, or
  detach input is supplied, then the corresponding policy state changes and an
  identifier-only success audit event is emitted.
- [x] Given insufficient permission or malformed, duplicate, missing,
  archived, or cross-workspace input, when a tool is called, then it returns a
  stable structured problem without leaking resource existence.
- [x] Given policy write permission, when mappings change, then the control's
  own metadata and framework/evidence mappings remain unchanged.
- [x] Given existing MCP control tools and permission scopes, when policy tools
  ship, then their names, schemas, and behavior remain unchanged.

**Tasks**

- [x] Add list/get/create/update/archive policy MCP DTOs, schemas, and handlers.
- [x] Add attach/detach policy-control MCP DTOs, schemas, and handlers.
- [x] Authorize reads with `read_controls` and mutations with `write_controls`.
- [x] Map validation, conflict, not-found, and dependency errors to established
  MCP problems.
- [x] Emit identifier-only MCP policy audit events.
- [x] Add tool-catalog and integration tests for success, ordering, schemas,
  permissions, tenancy, rollback, and audit behavior.

**Notes**

- The spec revision dated 2026-07-15 names the mutation `update_policy` because
  it changes name and description without replacing the policy or related state.
- The 2026-07-16 spec revision records the shipped nested attachment response
  and stable MCP policy audit event names, plus the entity/read-projection
  separation and dedicated `projections::policy_projection` module applied
  during review.
