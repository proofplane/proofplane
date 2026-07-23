# 005 — Unmap Control from Evidence

**Status:** Done · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add `unmap_control_from_evidence`, the mirror removal half: remove
the mappings between one control anchor and many evidence items in a single
transaction.

**Acceptance criteria**

- [x] Given a control ID and a list of mapped evidence IDs, when the tool is called, then every mapping is removed and the removed pairs are returned.
- [x] Given a batch containing an evidence item not currently mapped to that control, when the tool is called, then the call fails naming those evidence IDs and nothing is removed.
- [x] Given a control ID from another workspace, when the tool is called, then it is rejected as not found.
- [x] Given a successful batch, when it completes, then one `evidence_control_mappings.deleted` audit event is emitted whose `tool` field identifies this direction.

**Tasks**

- [x] Add the batch delete repository method anchored on the control.
- [x] Add the service method reusing the 004 short-count reporting.
- [x] Add the `#[tool]` and register it on the controls tool router.
- [x] Emit the batch audit event.
- [x] Integration tests covering success, not-mapped pair, and cross-workspace.

**Notes**

- A straight mirror of 004 — same one-statement delete-and-classify, same split
  between `unknown_ids` and `not_mapped_ids`, same `Error::BatchRejected` path
  so the rejection rolls the deletes back. No spec revision was needed.
- `EvidenceId` gained the `BatchKey` impl that `validate_batch` needs for a bare
  evidence-id list, mirroring `ControlId`'s.
