# 006 - MCP Metrics

**Status:** Todo · **Depends on:** mcp-server/005 · **Spec:** [spec.md](../spec.md#metrics-contract)

**Summary** - Complete MVP instrumentation with MCP tool outcome and duration
metrics.

**Acceptance criteria**

- [ ] Given MCP tool traffic, when metrics are read, then tool outcome and
  duration are labeled by stable tool name only.
- [ ] Given arbitrary actor, workspace, object, or error values, when metrics are
  rendered, then none appear as labels.
- [ ] Given existing API and worker metrics, when MCP metrics ship, then their
  names and labels remain unchanged.

**Tasks**

- [ ] Instrument MCP dispatch and problem outcomes.
- [ ] Add cardinality and representative-flow tests.
- [ ] Add the metrics to the deployment scrape documentation.
