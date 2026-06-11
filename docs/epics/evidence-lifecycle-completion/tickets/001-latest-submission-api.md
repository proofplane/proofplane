# 001 - Latest Submission API

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#api-contract)

**Summary** - Expose the existing latest-submission repository query through the
service and REST API so callers can inspect the current evidence response for an
Evidence Request.

**Acceptance criteria**

- [ ] Given multiple submissions, when latest is requested, then the newest by
  receipt time and ID is returned with its attachments.
- [ ] Given a missing, cross-workspace, unauthorized, or never-submitted
  Evidence Request, when latest is requested, then `404` is returned.
- [ ] Given existing submission create/get routes, when this ships, then their
  response shapes and authorization behavior are unchanged.

**Tasks**

- [ ] Add the service method and latest route.
- [ ] Reuse the existing submission-detail response mapping.
- [ ] Add API integration coverage for ordering and rejection cases.
- [ ] Confirm repository coverage remains aligned with the API contract.
