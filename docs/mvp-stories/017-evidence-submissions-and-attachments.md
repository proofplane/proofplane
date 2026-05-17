# 017 - Evidence Submissions and Attachments

## Goal

Allow actors to submit evidence against existing requirements, including attachment metadata and object storage references.

## Design

Evidence submissions support:

- requirement ID
- attachment reference, URL, text, or structured payload
- submitter actor
- actor type
- collection timestamp
- source system
- collection method
- provenance metadata
- checksum or hash
- approval status
- replacement or supplement relationship

Attachments use GCS/object storage for bytes and Postgres for metadata. Submissions inherit requirement-control mappings.

## Acceptance Criteria

- Submission and attachment tables are migrated.
- API supports creating a submission, uploading or registering an attachment, getting submission status, and retrieving latest submission for a requirement.
- Service validates requirement existence and workspace ownership.
- Submissions start in a pending approval status.
- Submission creation emits outbox and audit events.
- Seed data includes at least one pending submission and one attachment metadata record.

## Tests

- Domain tests cover status transitions and replacement/supplement rules.
- Repository integration tests cover submission creation and latest-submission queries.
- Storage integration tests cover attachment object upload and retrieval.
- API integration tests cover valid submission, invalid request accumulation, missing requirement, and latest submission.
- Tests verify submission inherits requirement mappings indirectly rather than copying mapping rows.

## QA Guide

1. Start API and storage emulator.
2. Submit evidence against a seeded requirement.
3. Upload or register an attachment.
4. Retrieve submission status.
5. Query latest submission for the requirement.
