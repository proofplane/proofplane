# 004 - Evidence Submission Context

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#evidence-submission-context)

**Summary** - Add immutable summary and description fields to an Evidence
Submission. The short summary supports compact agent workflows; the larger
description is available only when retrieving a submission directly by ID.

**Acceptance criteria**

- [ ] Given a valid summary and description when a submission is created, when
  that submission is retrieved by ID, then both immutable fields are returned.
- [ ] Given a latest-submission read, when a matching submission exists, then
  its summary is returned and its description is omitted.
- [ ] Given an omitted summary or description, when a submission is created or
  an existing submission is read, then the request remains valid and each
  omitted field is absent.
- [ ] Given a blank or over-limit field, when submission creation is attempted,
  then validation rejects it without persisting a submission.
- [ ] Given existing submission and attachment clients, when this ships, then
  their upload, retrieval, and immutable evidence behavior remains unchanged.

**Tasks**

- [ ] Add nullable summary and description columns with database length checks.
- [ ] Validate optional summaries as trimmed, non-blank text of at most 500
  characters and descriptions at most 4,000 characters.
- [ ] Accept both fields during submission creation and define separate compact
  and direct-detail response DTOs.
- [ ] Update repository mappings and deterministic seed data.
- [ ] Add route, repository, and integration coverage for field visibility,
  absence, and invalid values.

**Notes**

- These fields are evidence context supplied at submission time, not approval
  or mutable compliance conclusions.
- Packet previews, list-like reads, and latest-submission reads must never
  return the description.
- Full-text search and the previously proposed cross-record source-material
  model remain deferred.
