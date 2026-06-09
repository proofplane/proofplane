# 018 - Deferred Submission Approval and Control Status

## Goal

Defer Proofplane-owned submission approval and rejection until customer feedback
shows that an in-product approval workflow is needed.

## Design

For the MVP, Proofplane is approval-agnostic. A submission exists once the API
accepts it. Customers that need human or agent review can perform that review in
their own workflow before uploading evidence.

Control and source-material reads should treat submitted evidence as candidate
evidence, with file usability gated by attachment upload status. File
attachments are usable only when their status is `uploaded`; `pending`,
`finalizing`, `contains_virus`, and `failed` attachments remain blocked from
normal download and source-material use.

If customer feedback shows that Proofplane needs native approval, a future story
can introduce approval and rejection. That future design would likely derive
control status from:

- linked Evidence Requests
- latest usable submissions
- freshness and expiry rules
- missing evidence
- exceptions or emergency overrides later

Start with deterministic derived reads rather than denormalized status unless
performance requires projection later.

## Acceptance Criteria

- No approval or rejection API is required for the MVP.
- Evidence submission records do not need a persisted approval status.
- Control/source-material usability derives from Evidence Request mappings,
  submission freshness, and attachment upload status.
- `pending`, `finalizing`, `contains_virus`, or `failed` attachments block
  normal download and source-material use.
- Native approval/rejection can be added later without changing the story 017
  attachment lifecycle model.

## Tests

- No approval/rejection tests are required for the MVP.
- Future approval work should add domain tests for status transition rules.
- Future approval work should add service tests for authorization hooks and derived control status.
- Future approval work should add repository/API integration tests for approval, rejection, invalid transitions, control status, and evidence gaps.
- Time-dependent tests use fixed clocks.

## QA Guide

No MVP QA flow is required for native approval. Revisit this story after
customer feedback indicates that Proofplane should own approval state rather
than leaving review in caller workflows.
