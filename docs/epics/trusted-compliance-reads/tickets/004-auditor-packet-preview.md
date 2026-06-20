# 004 - Auditor Packet Preview

**Status:** Todo · **Depends on:** 003, evidence-lifecycle-completion/004, reliability-observability/005 · **Spec:** [spec.md](../spec.md#auditor-packet-read-model)

**Summary** - Assemble a compact JSON packet preview that explains how selected
controls map to requests, latest evidence state, and record provenance.

**Acceptance criteria**

- [ ] Given selected workspace controls, when preview is requested, then the
  complete mapped evidence graph and freshness state are returned.
- [ ] Given a control with missing or unusable evidence, when preview is
  requested, then the gap is explicit and no quarantined or persistent download
  link is exposed.
- [ ] Given submissions with summaries or descriptions, when preview is
  requested, then neither free-text field is included.
- [ ] Given a missing, cross-workspace, or unauthorized control, when preview is
  requested, then `404` is returned.
- [ ] Given a successful preview, when structured logs are inspected, then
  packet generation is attributable to the requesting actor.

**Tasks**

- [ ] Define packet DTOs and deterministic ordering.
- [ ] Add repository/service read composition.
- [ ] Add packet preview authorization and route.
- [ ] Emit a packet preview audit log.
- [ ] Add integration tests for complete and missing-evidence controls.

**Notes**

- The spec was revised on 2026-06-20 to use Evidence Submission summaries and
  defer the standalone source-material API.
