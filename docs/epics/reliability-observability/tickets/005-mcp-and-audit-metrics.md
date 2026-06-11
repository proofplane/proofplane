# 005 - MCP And Audit Metrics

**Status:** Todo · **Depends on:** mcp-server/004, audit-trail/003 · **Spec:** [spec.md](../spec.md#metrics-contract)

**Summary** - Complete MVP instrumentation with MCP tool and audit append/query
metrics.

**Acceptance criteria**

- [ ] Given MCP tool traffic, when metrics are read, then tool outcome and
  duration are labeled by stable tool name only.
- [ ] Given audit append/query activity, when metrics are read, then success,
  failure, and duration signals are present without event payload data.
- [ ] Given arbitrary actor, workspace, object, or error values, when metrics are
  rendered, then none appear as labels.

**Tasks**

- [ ] Instrument MCP dispatch and problem outcomes.
- [ ] Instrument audit append and query operations.
- [ ] Add cardinality and representative-flow tests.
- [ ] Add the metrics to the deployment scrape documentation.
