# 012 - Transactional Outbox

## Goal

Persist domain events in the database transaction that changes domain state, then publish them asynchronously.

## Design

Add an `outbox_messages` table with:

- ID
- topic
- event type
- aggregate type
- aggregate ID
- payload JSON
- attributes JSON
- status
- attempt count
- next available at
- locked by
- locked at
- created at
- published at
- last error

Repository APIs should support appending outbox records inside existing transactions and claiming batches safely for concurrent workers.

## Acceptance Criteria

- Domain state changes and outbox insertions commit atomically.
- Dequeuer can claim pending rows without double-sending under concurrent workers.
- Successfully published records are marked sent.
- Failed records are retried with backoff and eventually marked failed or dead-lettered according to policy.
- Outbox schema is migrated and seeded where useful.

## Tests

- Repository integration tests verify atomic commit and rollback.
- Concurrent claim tests verify two workers do not claim the same message.
- Retry scheduling tests verify attempt counts and `next_available_at`.
- Service tests verify domain operations append expected outbox messages.

## QA Guide

1. Run migrations.
2. Trigger a domain write that emits an event.
3. Query `outbox_messages` and confirm a pending row exists in the same transaction.
4. Run the dequeuer and confirm the row transitions to sent.
