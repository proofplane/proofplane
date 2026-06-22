# 004 - Evidence Submission Context

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#evidence-submission-context)

**Summary** - Add immutable summary and description fields to an Evidence
Submission. The short summary supports compact agent workflows; the larger
description is available only when retrieving a submission directly by ID.

**Acceptance criteria**

- [x] Given a valid summary and description when a submission is created, when
  that submission is retrieved by ID, then both immutable fields are returned.
- [x] Given a latest-submission read, when a matching submission exists, then
  its summary is returned and its description is omitted.
- [x] Given an omitted summary or description, when a submission is created or
  an existing submission is read, then the request remains valid and each
  omitted field is absent.
- [x] Given a blank or over-limit field, when submission creation is attempted,
  then validation rejects it without persisting a submission.
- [x] Given existing submission and attachment clients, when this ships, then
  their upload, retrieval, and immutable evidence behavior remains unchanged.

**Tasks**

- [x] Add nullable summary and description columns with database length checks.
- [x] Validate optional summaries as trimmed, non-blank text of at most 500
  characters and descriptions at most 4,000 characters.
- [x] Accept both fields during submission creation and define separate compact
  and direct-detail response DTOs.
- [x] Update repository mappings and deterministic seed data.
- [x] Add route, repository, and integration coverage for field visibility,
  absence, and invalid values.

**Notes**

- These fields are evidence context supplied at submission time, not approval
  or mutable compliance conclusions.
- Packet previews, list-like reads, and latest-submission reads must never
  return the description.
- Full-text search and the previously proposed cross-record source-material
  model remain deferred.
