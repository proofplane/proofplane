# 007 - Agent Connections

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 006 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Convert the agent-connection lifecycle to explicit commands and
separate reusable-connection and authority-resolution read models.

**Acceptance criteria**

- [ ] Given each valid lifecycle state, when its command is handled, then the connection snapshot makes exactly one allowed transition.
- [ ] Given expired, consumed, revoked, or unauthorized context, when handled, then the command is rejected without events.
- [ ] Given existing OAuth/MCP callers, when cut over, then permission and concealment behavior remain unchanged.

**Tasks**

- [ ] Complete aggregate lifecycle and permission behavior.
- [ ] Add request, deny, consume, activate, use, authorize, and revoke handlers.
- [ ] Add reusable-connection and authority query handlers.
- [ ] Migrate OAuth, MCP, and route callers.
- [ ] Add lifecycle, expiry, concurrency, and query tests.
