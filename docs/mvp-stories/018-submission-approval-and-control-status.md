# 018 - Submission Approval and Control Status

## Goal

Support approval and rejection of evidence submissions and derive control status from mapped requirements and approved evidence.

## Design

Authorized actors can approve or reject pending submissions. Approval means the submission satisfies the requirement for the relevant period. Control status is derived from:

- linked Evidence Requests
- latest approved submissions
- freshness and expiry rules
- missing evidence
- exceptions or emergency overrides later

Start with deterministic derived reads rather than denormalized status unless performance requires projection later.

## Acceptance Criteria

- API supports approve submission, reject submission, get control status, and get control evidence gaps.
- Approval and rejection require authenticated actor context.
- Invalid transitions are rejected with stable errors.
- Control status reflects approved, pending, stale, expired, and missing evidence.
- Approval and rejection emit outbox and audit events.
- Seed data includes approved, pending, stale, and missing examples.

## Tests

- Domain tests cover status transition rules.
- Service tests cover authorization hooks and derived control status.
- Repository integration tests cover latest approved submission by requirement.
- API integration tests cover approve, reject, invalid transition, control status, and evidence gaps.
- Time-dependent tests use fixed clocks.

## QA Guide

1. Submit pending evidence.
2. Approve it with an authorized seeded actor.
3. Get the related control status and confirm the requirement is satisfied.
4. Reject another pending submission and confirm it does not satisfy the control.
5. Advance or simulate time to verify stale or expired status.
