# 005 - Portal Read Model

**Status:** Todo · **Depends on:** 004 · **Spec:** [spec.md](../spec.md#portal-read-model)

**Summary** - Build the workspace-wide read model auditors see in the portal:
all controls, mapped evidence requests, all submissions, and attachment
metadata.

**Acceptance criteria**

- [ ] Given a valid auditor session, when portal data is requested, then all
  controls, mapped evidence requests, all submissions, and attachment metadata
  are returned in deterministic order.
- [ ] Given unavailable attachments, when the model is assembled, then their
  lifecycle status is visible but no download action is available.
- [ ] Given a missing, expired, revoked, or cross-workspace session, when portal
  data is requested, then workspace data is not returned.
- [ ] Given portal metadata, when inspected, then object keys and storage backend
  details are absent.

**Tasks**

- [ ] Add repository/service read composition for the portal graph.
- [ ] Include all historical submissions, not latest-only submissions.
- [ ] Mark attachment download eligibility.
- [ ] Add deterministic ordering.
- [ ] Add workspace isolation, unavailable-attachment, and object-key exclusion
  tests.
