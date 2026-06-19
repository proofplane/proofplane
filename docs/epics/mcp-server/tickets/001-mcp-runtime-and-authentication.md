# 001 - MCP Runtime And Authentication

**Status:** Todo · **Depends on:** paseto-token-migration/006 · **Spec:** [spec.md](../spec.md#runtime-and-transport)

**Summary** - Replace the existing scaffold with a streamable-HTTP MCP runtime
that authenticates user-owned workspace API tokens and shuts down cleanly.

**Acceptance criteria**

- [ ] Given valid config and dependencies, when the MCP binary starts, then it
  binds `mcp_bind`, serves protocol initialization, health, and metrics.
- [ ] Given a missing, revoked, expired, or cross-workspace API token, when a tool
  is invoked, then authentication fails without running the tool.
- [ ] Given dependency initialization failure, when startup occurs, then the
  process exits before accepting sessions with an actionable error.
- [ ] Given a shutdown signal, when received, then active requests receive the
  configured graceful shutdown behavior.

**Tasks**

- [ ] Select and integrate the MCP SDK.
- [ ] Build runtime dependency composition in the binary.
- [ ] Add opaque `ppat_` bearer session authentication and `ApiTokenContext`.
- [ ] Add health, metrics, and graceful shutdown.
- [ ] Add protocol startup/auth integration tests.

**Notes**

- Revised with the 2026-06-19 API-token spec pivot: MCP waits for ticket 006
  and consumes the same opaque bearer contract as REST.
