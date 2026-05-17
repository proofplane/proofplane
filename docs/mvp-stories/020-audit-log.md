# 020 - Audit Log

## Goal

Record and query meaningful reads, writes, approvals, mappings, retrievals, and agent actions.

## Design

Audit events should include:

- actor
- actor type
- timestamp
- action
- object touched
- previous state where relevant
- new state where relevant
- rationale
- source request or session
- API/MCP client identity
- correlation ID

Audit logging should be called by services for business events and middleware for request metadata. Avoid logging sensitive request payloads or object bytes.

## Acceptance Criteria

- Audit event table is migrated.
- Services write audit events for create, update, mapping, submission, approval, rejection, source material retrieval, and emergency actions when available.
- API supports querying audit history with filters.
- MCP later reuses the same service.
- Audit writes participate in the same transaction as state changes where consistency matters.
- Seed data includes representative audit history.

## Tests

- Repository integration tests cover append and filtered query.
- Service tests verify expected audit records for business operations.
- API integration tests cover audit query filters and auth.
- Tests verify sensitive headers and credentials are never persisted.
- Transaction tests verify rollback also rolls back related audit records.

## QA Guide

1. Perform a seeded authenticated API operation.
2. Query audit history for the actor.
3. Query audit history for the touched object.
4. Confirm old and new state are present where expected.
5. Confirm secrets and attachment bytes are absent.
