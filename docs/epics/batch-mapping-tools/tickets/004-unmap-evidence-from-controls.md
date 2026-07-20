# 004 — Unmap Evidence from Controls

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add `unmap_evidence_from_controls`: remove the mappings between one
evidence anchor and many controls in a single transaction, so unwinding a
mis-mapped evidence item is one call rather than one per control.

**Acceptance criteria**

- [ ] Given an evidence ID and a list of mapped control IDs, when the tool is called, then every mapping is removed and the removed pairs are returned.
- [ ] Given a batch containing a control that is not currently mapped to that evidence, when the tool is called, then the call fails naming those control IDs and no mapping is removed.
- [ ] Given an evidence ID from another workspace, when the tool is called, then it is rejected as not found and nothing is removed.
- [ ] Given a successful batch, when it completes, then one `evidence_control_mappings.deleted` audit event is emitted with the evidence ID, control ID list, and count.
- [ ] Given the existing `remove_evidence_control_mapping` tool, when this ships, then its behavior is unchanged.

**Tasks**

- [ ] Add the batch delete repository method returning the removed pairs.
- [ ] Fail the batch when the removed count is short of the requested count, identifying which pairs were not mapped.
- [ ] Add the service method and the `#[tool]`, registered on the controls tool router.
- [ ] Emit the batch audit event.
- [ ] Integration tests covering success, not-mapped pair, cross-workspace, and rollback.

**Notes**

- Removal is not idempotent by design: unmapping a pair that is not mapped fails
  the batch. See the spec's Already-mapped pairs section.
