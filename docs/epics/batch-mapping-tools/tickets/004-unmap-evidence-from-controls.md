# 004 — Unmap Evidence from Controls

**Status:** Done · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add `unmap_evidence_from_controls`: remove the mappings between one
evidence anchor and many controls in a single transaction, so unwinding a
mis-mapped evidence item is one call rather than one per control.

**Acceptance criteria**

- [x] Given an evidence ID and a list of mapped control IDs, when the tool is called, then every mapping is removed and the removed pairs are returned.
- [x] Given a batch containing a control that is not currently mapped to that evidence, when the tool is called, then the call fails naming those control IDs and no mapping is removed.
- [x] Given an evidence ID from another workspace, when the tool is called, then it is rejected as not found and nothing is removed.
- [x] Given a successful batch, when it completes, then one `evidence_control_mappings.deleted` audit event is emitted with the evidence ID, control ID list, and count.
- [x] Given the existing `remove_evidence_control_mapping` tool, when this ships, then its behavior is unchanged.

**Tasks**

- [x] Add the batch delete repository method returning the removed pairs.
- [x] Fail the batch when the removed count is short of the requested count, identifying which pairs were not mapped.
- [x] Add the service method and the `#[tool]`, registered on the controls tool router.
- [x] Emit the batch audit event.
- [x] Integration tests covering success, not-mapped pair, cross-workspace, and rollback.

**Notes**

- Removal is not idempotent by design: unmapping a pair that is not mapped fails
  the batch. See the spec's Already-mapped pairs section.
- A control that is not mapped and a control that does not exist get separate
  codes — `not_mapped_ids` and `unknown_ids`. One statement deletes and
  classifies, so a rejection is found after the write and travels as
  `Error::BatchRejected` to roll the batch back. Both are recorded in the spec's
  ticket-004 revision notes; 005 and 008 should follow the same shape.
