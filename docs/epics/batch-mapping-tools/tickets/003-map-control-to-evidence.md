# 003 — Map Control to Evidence

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#core-principle-batches-go-one-way)

**Summary** — Add `map_control_to_evidence`, the mirror half of the
evidence ↔ control relationship: one control anchor fanned out to many evidence
items, each with its own rationale, in a single transaction.

**Acceptance criteria**

- [ ] Given a control ID and several `{evidence_id, rationale}` items, when the tool is called, then every mapping is created and all of them are returned.
- [ ] Given a batch containing an evidence ID that does not exist in the workspace, when the tool is called, then the call fails naming every unknown evidence ID and nothing is written.
- [ ] Given a batch containing a pair that is already mapped, when the tool is called, then the call fails and nothing is written.
- [ ] Given a control ID from another workspace, when the tool is called, then it is rejected as not found.
- [ ] Given a successful batch, when it completes, then one `evidence_control_mappings.created` audit event is emitted whose `tool` field identifies this direction.

**Tasks**

- [ ] Add the batch insert repository method anchored on the control.
- [ ] Add the service method reusing the 001 validation and unknown-ID reporting.
- [ ] Add the `#[tool]` and register it on the controls tool router.
- [ ] Emit the batch audit event.
- [ ] Integration tests covering success, unknown IDs, duplicate pair, and cross-workspace.

**Notes**

- Shares the audit event name with 002; direction is recoverable from the `tool`
  field — see the spec's Audit events table.
