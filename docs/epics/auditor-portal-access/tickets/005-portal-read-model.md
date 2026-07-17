# 005 - Portal Read Model

**Status:** Done · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#portal-read-model)

**Summary** - Build the workspace-wide read model auditors see in the portal:
all controls, mapped evidence, all submissions, and file
metadata.

**Acceptance criteria**

- [x] Given a valid auditor session, when portal data is requested, then all
  controls, mapped evidence, all submissions, and file metadata
  are returned in deterministic order.
- [x] Given unavailable submissions, when the model is assembled, then their
  lifecycle status is visible but no download action is available.
- [x] Given a missing, expired, revoked, or cross-workspace session, when portal
  data is requested, then workspace data is not returned.
- [x] Given portal metadata, when inspected, then object keys and storage backend
  details are absent.

**Tasks**

- [x] Add repository/service read composition for the portal graph.
- [x] Include all historical submissions, not latest-only submissions.
- [x] Mark submission download eligibility.
- [x] Add deterministic ordering.
- [x] Add workspace isolation, unavailable-submission, and object-key exclusion
  tests.

**Notes**

- Spec revised to record the shipped `/auditor-access/portal/data` endpoint and
  archived submission filtering.
