# 001 - MCP Runtime And Authentication

**Status:** Todo · **Depends on:** auth-hierarchy-api/003 · **Spec:** [spec.md](../spec.md#runtime-and-transport)

**Summary** - Replace the exiting scaffold with a streamable-HTTP MCP runtime
that authenticates workspace actors and shuts down cleanly.

**Acceptance criteria**

- [ ] Given valid config and dependencies, when the MCP binary starts, then it
  binds `mcp_bind`, serves protocol initialization, health, and metrics.
- [ ] Given a missing, revoked, expired, or cross-workspace API key, when a tool
  is invoked, then authentication fails without running the tool.
- [ ] Given dependency initialization failure, when startup occurs, then the
  process exits before accepting sessions with an actionable error.
- [ ] Given a shutdown signal, when received, then active requests receive the
  configured graceful shutdown behavior.

**Tasks**

- [ ] Select and integrate the MCP SDK.
- [ ] Build runtime dependency composition in the binary.
- [ ] Add API-key session authentication and `ActorContext`.
- [ ] Add health, metrics, and graceful shutdown.
- [ ] Add protocol startup/auth integration tests.
