# 008 — Detach Policy and Control Batches

**Status:** Todo · **Depends on:** 006, 007 · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add both removal halves for policy ↔ control:
`detach_policy_from_controls` (one policy → many controls) and
`detach_control_from_policies` (one control → many policies). They ship together
because, with no rationale and no per-pair payload, each is a thin mirror of its
attach counterpart.

**Acceptance criteria**

- [ ] Given a policy ID and a list of attached control IDs, when `detach_policy_from_controls` is called, then every mapping is removed and the removed pairs are returned.
- [ ] Given a control ID and a list of policies it is attached to, when `detach_control_from_policies` is called, then every mapping is removed and the removed pairs are returned.
- [ ] Given a batch containing a pair that is not currently attached, when either tool is called, then the call fails naming those IDs and no mapping is removed.
- [ ] Given an archived policy, when either tool is called with it, then it is rejected and nothing is removed.
- [ ] Given a successful batch, when it completes, then one `policy_control_mappings.deleted` audit event is emitted for the whole batch.
- [ ] Given the existing `detach_policy_from_control` tool, when this ships, then its behavior is unchanged.

**Tasks**

- [ ] Add both batch delete repository methods, preserving the archived-policy guard.
- [ ] Fail each batch on a short delete count, identifying the unattached pairs.
- [ ] Add both service methods and both `#[tool]`s, registered on the policies tool router.
- [ ] Emit the batch audit events.
- [ ] Integration tests for both directions covering success, not-attached pair, archived policy, and rollback.
