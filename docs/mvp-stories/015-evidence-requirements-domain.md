# 015 - Evidence Requests Domain

## Goal

Implement Evidence Request domain, persistence, service, and API behavior.

An Evidence Request is Proofplane's scheduled request for proof that a workspace is meeting an external requirement. Uploaded evidence, attachment bytes, approval, and freshness/staleness based on submissions are later concerns.

## Slicing

Slice 1 includes:

- domain model
- database migration
- repository methods
- seed data

Slice 2 includes:

- API endpoints
- service orchestration

Later story 017:

- submissions and attachments
- effective or coverage date
- stale status
- uploaded file/blob semantics

Explicit slice 1 decisions:

- no evidence type
- no owner or team
- no source system
- no API
- no auth
- no stale query

Explicit slice 2 decisions:

- add create, get, list by workspace, full replace update, and list due API endpoints
- route workspace scope through a path workspace UUID until auth middleware exists
- keep stale query, submissions, attachments, owner/team, source-system, and uploaded-evidence semantics out of story 015

## Design

Evidence Requests are durable scheduled objects describing what evidence should be collected. Include:

- title
- description
- collection instructions
- cadence
- due date
- schedule anchor date
- optional freshness window days
- status
- workspace ID

Cadence is schedule-owned through `schedule_anchor_at`. Late submissions must not shift future due dates.

Repository methods should support create, list by workspace, get, full replace update, and list due. `list_due` returns active Evidence Requests where `due_at <= now`, ordered by `due_at`.

API endpoints should support create, list by workspace, get, full replace update, and list due. Request handlers validate DTOs with the applicative validation macro, map to domain types, invoke services, and map results to response DTOs.

## Acceptance Criteria

- Evidence Request table is migrated.
- Repository supports create, get, list by workspace, full replace update, and list due.
- Domain model enforces Evidence Request invariants.
- Service validates Evidence Request input and delegates persistence through the repository trait.
- API exposes Evidence Request endpoints.
- Seed data includes realistic demo Evidence Requests.
- No auth behavior, stale query, owner/team, source-system, or uploaded-evidence semantics are introduced in story 015.

## Tests

- Domain unit tests cover parsing and invariants.
- Validation tests cover accumulated field errors.
- Repository code is compile-checked.
- API and service code are compile-checked.
- Database-backed repository tests remain deferred until story 009 introduces the integration harness.

## QA Guide

1. Run migrations and seed.
2. Start API.
3. List seeded Evidence Requests for the local workspace UUID.
4. Create an Evidence Request with multiple invalid fields and confirm field errors return.
5. Create a valid Evidence Request and retrieve it by ID.
