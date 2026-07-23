# 007 — Attach Control to Policies

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#core-principle-batches-go-one-way)

**Summary** — Add `attach_control_to_policies`, the mirror half of
policy ↔ control: attach one control anchor to many active policies in a single
transaction, for the agent that has just created a control and knows which
policies govern it.

**Acceptance criteria**

- [x] Given a control ID and a list of policy IDs, when the tool is called, then every mapping is created and all of them are returned.
- [x] Given a batch containing a policy ID that does not exist in the workspace, when the tool is called, then the call fails naming every unknown policy ID and nothing is written.
- [x] Given a batch containing an archived policy, when the tool is called, then the call fails identifying it and nothing is written.
- [x] Given a control ID from another workspace, when the tool is called, then it is rejected as not found.
- [x] Given a successful batch, when it completes, then one `policy_control_mappings.created` audit event is emitted whose `tool` field identifies this direction.

**Tasks**

- [x] Add the batch insert repository method anchored on the control, excluding archived policies.
- [x] Report archived policies distinctly from unknown ones so the agent can tell the two apart.
- [x] Add the service method and the `#[tool]`, registered on the policies tool router.
- [x] Emit the batch audit event.
- [x] Integration tests covering success, unknown IDs, archived policy, and cross-workspace.
