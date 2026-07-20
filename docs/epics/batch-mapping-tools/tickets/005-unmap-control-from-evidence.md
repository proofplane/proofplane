# 005 — Unmap Control from Evidence

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add `unmap_control_from_evidence`, the mirror removal half: remove
the mappings between one control anchor and many evidence items in a single
transaction.

**Acceptance criteria**

- [ ] Given a control ID and a list of mapped evidence IDs, when the tool is called, then every mapping is removed and the removed pairs are returned.
- [ ] Given a batch containing an evidence item not currently mapped to that control, when the tool is called, then the call fails naming those evidence IDs and nothing is removed.
- [ ] Given a control ID from another workspace, when the tool is called, then it is rejected as not found.
- [ ] Given a successful batch, when it completes, then one `evidence_control_mappings.deleted` audit event is emitted whose `tool` field identifies this direction.

**Tasks**

- [ ] Add the batch delete repository method anchored on the control.
- [ ] Add the service method reusing the 004 short-count reporting.
- [ ] Add the `#[tool]` and register it on the controls tool router.
- [ ] Emit the batch audit event.
- [ ] Integration tests covering success, not-mapped pair, and cross-workspace.
