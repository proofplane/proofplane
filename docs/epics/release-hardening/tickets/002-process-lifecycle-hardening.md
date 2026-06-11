# 002 - Process Lifecycle Hardening

**Status:** Todo · **Depends on:** mcp-server/001 · **Spec:** [spec.md](../spec.md#process-contract)

**Summary** - Standardize startup failure, health, metrics, and graceful shutdown
behavior across every long-running binary.

**Acceptance criteria**

- [ ] Given invalid config or unavailable required startup dependencies, when a
  process starts, then it exits non-zero before serving or polling.
- [ ] Given a shutdown signal, when each process receives it, then new work stops
  and in-flight work gets a bounded grace period.
- [ ] Given deployment probes, when each process is healthy or unhealthy, then
  documented liveness/readiness behavior is observable.
- [ ] Given existing API/worker routes, when this ships, then their paths remain
  compatible.

**Tasks**

- [ ] Audit API, worker, dequeuer, and MCP startup/shutdown behavior.
- [ ] Consume or remove currently unused lifecycle configuration.
- [ ] Add binary smoke tests for start and stop.
- [ ] Document per-process health contracts.
