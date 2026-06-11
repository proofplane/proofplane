# 004 - MCP Audit And Equivalence

**Status:** Todo · **Depends on:** 002, 003, audit-trail/003 · **Spec:** [spec.md](../spec.md#audit)

**Summary** - Make MCP activity attributable and prove that shared REST/MCP
operations have equivalent domain outcomes.

**Acceptance criteria**

- [ ] Given a meaningful MCP read or write, when it completes, then audit
  history records actor, workspace, client type, tool, and correlation context.
- [ ] Given `log_agent_action`, when valid input is submitted, then an allowlisted
  action event is stored without arbitrary secret-bearing payload fields.
- [ ] Given matched REST and MCP scenarios, when executed, then persisted domain
  state and authorization outcomes are equivalent.
- [ ] Given a tool failure or rollback, when history is inspected, then no
  success event is present.

**Tasks**

- [ ] Add MCP audit context and `log_agent_action`.
- [ ] Add cross-interface contract fixtures.
- [ ] Test read, write, denial, validation, and rollback equivalence.
- [ ] Document intentional protocol-shape differences.
