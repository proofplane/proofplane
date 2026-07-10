# 005 - MCP Logging And Equivalence

**Status:** Needs rework · **Depends on:** 002, 003, 004, reliability-observability/005, reliability-observability/007 · **Spec:** [spec.md](../spec.md#audit)

**Summary** - Emit attributable structured logs for MCP activity. The original
REST/MCP equivalence goal is obsolete: the REST data-plane was removed in PR #42
(see the spec's 2026-07-09 reconciliation banner), so MCP is the only
compliance interface and there is nothing to prove parity against. This ticket
should be reframed to MCP-only audit coverage before it is picked up.

**Acceptance criteria**

- [ ] Given a meaningful MCP read or write, when it completes, then audit
  logs record the actor (agent connection / user), workspace, client type,
  tool, and correlation context.
- [ ] Given a tool failure or rollback, when logs are inspected, then no success
  audit log is present.
- [ ] Given any tool input, when logs are inspected, then credentials,
  free-form submission context, grant URLs, object keys, and packet bytes are
  absent.

**Tasks**

- [ ] Reframe this ticket away from REST/MCP equivalence (REST removed in PR #42).
- [ ] Add MCP audit-log context to tool dispatch.
- [ ] Test read, write, denial, validation, and rollback audit behavior.
- [ ] Test the audit-log field contract and secret exclusions.

**Notes**

- 2026-07-09: Actor is now the agent connection (OAuth), not an API token;
  the "API token" audit field references are superseded.
