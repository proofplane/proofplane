# 003 - Compliance Write Tools

**Status:** Todo · **Depends on:** 001, trusted-compliance-reads/002 · **Spec:** [spec.md](../spec.md#mvp-tools)

**Summary** - Add service-backed tools for submission records, control mappings,
and curated source material while leaving binary file transfer to REST.

**Acceptance criteria**

- [ ] Given valid authorized input, when a write tool runs, then the same domain
  state and audit event as the equivalent REST operation are produced.
- [ ] Given invalid links, duplicate mappings, or validation failures, when a
  write tool runs, then a stable structured problem is returned.
- [ ] Given an unauthorized actor, when a write tool runs, then no state or
  audit success event is committed.
- [ ] Given submission creation through MCP, when it succeeds, then the result
  explains the REST attachment upload contract without accepting file bytes.

**Tasks**

- [ ] Add submission and mapping tools.
- [ ] Add source-material create/update tool.
- [ ] Reuse service DTO mapping and validation semantics.
- [ ] Add write, rejection, and rollback integration tests.
