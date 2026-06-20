# 004 - MCP Logging And Equivalence

**Status:** Todo · **Depends on:** 002, 003, reliability-observability/005 · **Spec:** [spec.md](../spec.md#audit)

**Summary** - Emit attributable structured logs for MCP activity and prove that
shared REST/MCP operations have equivalent domain outcomes.

**Acceptance criteria**

- [ ] Given a meaningful MCP read or write, when it completes, then audit
  logs record user, API token, workspace, client type, tool, and correlation
  context.
- [ ] Given matched REST and MCP scenarios, when executed, then persisted domain
  state and authorization outcomes are equivalent.
- [ ] Given a tool failure or rollback, when logs are inspected, then no success
  audit log is present.
- [ ] Given any tool input, when logs are inspected, then credentials and
  free-form source-material content are absent.

**Tasks**

- [ ] Add MCP audit-log context to tool dispatch.
- [ ] Add cross-interface contract fixtures.
- [ ] Test read, write, denial, validation, and rollback equivalence.
- [ ] Test the audit-log field contract and secret exclusions.
- [ ] Document intentional protocol-shape differences.
