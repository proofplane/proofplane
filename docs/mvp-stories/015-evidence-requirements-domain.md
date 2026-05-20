# 015 - Evidence Requests Domain

## Goal

Implement the non-API foundation for Evidence Requests.

An Evidence Request is Proofplane's scheduled request for proof that a workspace is meeting an external requirement. Uploaded evidence, attachment bytes, approval, and freshness/staleness based on submissions are later concerns.

## Slicing

Slice 1 includes:

- domain model
- database migration
- repository methods
- seed data

Later slice:

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

## Acceptance Criteria

- Evidence Request table is migrated.
- Repository supports create, get, list by workspace, full replace update, and list due.
- Domain model enforces slice 1 invariants.
- Seed data includes realistic demo Evidence Requests.
- No API endpoints, auth behavior, service orchestration, stale query, owner/team, source-system, or uploaded-evidence semantics are introduced in this slice.

## Tests

- Domain unit tests cover parsing and invariants.
- Repository code is compile-checked.
- Database-backed repository tests remain deferred until story 009 introduces the integration harness.

## QA Guide

1. Run migrations and seed.
2. Confirm the `evidence_requests` table exists.
3. Confirm seeded local Evidence Requests exist for the workspace with slug `local-workspace`.
