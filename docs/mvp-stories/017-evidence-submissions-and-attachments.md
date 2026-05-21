# 017 - Evidence Submissions and Attachments

## Goal

Allow actors to submit evidence against existing Evidence Requests, including attachment metadata and object storage references.

## Design

Evidence submissions support:

- Evidence Request ID
- attachment reference, URL, text, or structured payload
- submitter actor
- actor type
- system receipt timestamp
- evidence effective or coverage date
- source system
- collection method
- provenance metadata
- checksum or hash
- approval status
- replacement or supplement relationship

Attachments use GCS/object storage for bytes and Postgres for metadata. Submissions inherit Evidence Request-control mappings.

Submissions must distinguish system receipt time from the evidence effective or coverage date. Late submissions must not shift future Evidence Request due dates; cadence remains schedule-owned through the request's `schedule_anchor_at`.

## Acceptance Criteria

- Submission and attachment tables are migrated.
- API supports creating a submission, uploading or registering an attachment, getting submission status, and retrieving latest submission for a requirement.
- Service validates Evidence Request existence and workspace ownership.
- Submissions start in a pending approval status.
- Submission creation emits outbox and audit events.
- Seed data includes at least one pending submission and one attachment metadata record.

## Tests

- Domain tests cover status transitions and replacement/supplement rules.
- Repository integration tests cover submission creation and latest-submission queries.
- Storage integration tests cover attachment object upload and retrieval.
- API integration tests cover valid submission, invalid request accumulation, missing Evidence Request, and latest submission.
- Tests verify submission inherits Evidence Request mappings indirectly rather than copying mapping rows.

## QA Guide

1. Start API with filesystem-backed object storage config.
2. Submit evidence against a seeded Evidence Request.
3. Upload or register an attachment.
4. Retrieve submission status.
5. Query latest submission for the requirement.
