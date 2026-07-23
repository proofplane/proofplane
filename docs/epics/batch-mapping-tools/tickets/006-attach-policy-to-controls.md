# 006 — Attach Policy to Controls

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#implementation-shape)

**Summary** — Add `attach_policy_to_controls`: attach one active policy anchor to
many controls in a single transaction, so governing a set of controls with a
newly authored policy is one call.

**Acceptance criteria**

- [x] Given a policy ID and a list of control IDs, when the tool is called, then every mapping is created and all of them are returned.
- [x] Given a batch containing a control ID that does not exist in the workspace, when the tool is called, then the call fails naming every unknown control ID and nothing is written.
- [x] Given an archived policy, when the tool is called, then it is rejected and nothing is written.
- [x] Given a batch containing a pair that is already attached, when the tool is called, then the call fails and nothing is written.
- [x] Given a successful batch, when it completes, then one `policy_control_mappings.created` audit event is emitted with the policy ID, control ID list, and count.

**Tasks**

- [x] Add the batch insert repository method, preserving the existing `archived_at IS NULL` guard.
- [x] Add the service method reusing the 001 validation and unknown-ID reporting.
- [x] Add the `#[tool]` and register it on the policies tool router.
- [x] Emit the batch audit event.
- [x] Integration tests covering success, unknown IDs, archived policy, duplicate pair, and cross-workspace.

**Notes**

- Takes a bare control ID list, not objects — policy ↔ control mappings carry no
  rationale. See the spec's Implementation shape section.
