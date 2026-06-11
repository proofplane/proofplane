# 001 - End-To-End MVP Flow

**Status:** Todo · **Depends on:** evidence-lifecycle-completion/003, trusted-compliance-reads/005, mcp-server/004, audit-trail/003 · **Spec:** [spec.md](../spec.md#release-flow)

**Summary** - Add one scripted and automated flow from workspace actor creation
through evidence processing, trusted reads, packet export, MCP, and audit.

**Acceptance criteria**

- [ ] Given clean local dependencies, when the flow runs, then a finalized
  attachment is downloadable and included in an auditor packet.
- [ ] Given the same records, when REST and MCP reads run, then they return
  equivalent domain outcomes.
- [ ] Given the completed flow, when audit and metrics are inspected, then the
  expected operation chain and runtime signals are present.
- [ ] Given a malicious test attachment in a local-only scenario, when processed,
  then it is absent from download and packet export.

**Tasks**

- [ ] Add deterministic demo orchestration and fixtures.
- [ ] Add focused end-to-end integration target.
- [ ] Assert outbox, worker, storage, audit, packet, and MCP outcomes.
- [ ] Document the manual demonstration commands.
