# 003 - Compliance Write Tools

**Status:** Done · **Depends on:** 001, evidence-lifecycle-completion/004, reliability-observability/007 · **Spec:** [spec.md](../spec.md#core-demo-tools)

**Summary** - Add service-backed tools for submission records with bounded
context and for control mappings while leaving binary file transfer to REST.

**Acceptance criteria**

- [x] Given valid authorized input, when a write tool runs, then the same domain
  state and structured audit log as the equivalent REST operation are produced.
- [x] Given invalid links, duplicate mappings, or validation failures, when a
  write tool runs, then a stable structured problem is returned.
- [x] Given an unauthorized user API token, when a write tool runs, then no
  state or success audit log is produced.
- [x] Given submission creation through MCP, when it succeeds, then the result
  returns the submission ID and compact REST attachment-upload instructions
  without echoing its optional summary or description or accepting file bytes.

**Tasks**

- [x] Add submission and mapping tools.
- [x] Reuse service DTO mapping and validation semantics.
- [x] Add write, rejection, and rollback integration tests.

**Notes**

- The spec was revised on 2026-06-20 to defer standalone source-material
  curation.
