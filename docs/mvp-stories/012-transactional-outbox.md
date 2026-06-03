# 012 - Transactional Outbox Publish Dequeuer

## Goal

Persist domain events in the database transaction that changes domain state, publish them asynchronously to Pub/Sub, and delete rows only after publish succeeds.

## Design

Add an `outbox_messages` table with:

- `id BIGSERIAL PRIMARY KEY`
- `topic TEXT NOT NULL`
- `event_type TEXT NOT NULL`
- `aggregate_type TEXT NOT NULL`
- `aggregate_id TEXT NOT NULL`
- `payload JSONB NOT NULL`
- `attributes JSONB NOT NULL DEFAULT '{}'::jsonb`
- `attempt_count INTEGER NOT NULL DEFAULT 0`
- `next_available_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Repository APIs support appending outbox records inside existing transactions, listing due rows in stable `next_available_at, id` order, deleting rows after successful publish, and recording publish failures by incrementing `attempt_count` and scheduling `next_available_at`.

The Pub/Sub side exposes a static-dispatch publisher trait for `OutboundMessage` and `TopicName`. The single-process dequeuer polls due rows, publishes each payload to its configured topic, adds outbox metadata attributes (`outbox_message_id`, `event_type`, `aggregate_type`, `aggregate_id`), deletes only after publish success, and retries failures with backoff while the row remains in the table.

Duplicate publishes are acceptable if the process crashes after publish but before delete; downstream handlers must be idempotent. MVP assumes one dequeuer process, so there are intentionally no claim or lock columns.

## Acceptance Criteria

- Domain state changes and outbox insertions commit atomically.
- Due rows are listed in stable order by `next_available_at, id`.
- Successfully published records are deleted after publish returns success.
- Failed publishes increment `attempt_count` and schedule `next_available_at`; failure detail is emitted through logs.
- Outbox schema is migrated without delivery status, claim, lock, or published-at columns.

## Tests

- Repository integration tests verify atomic commit and rollback of domain write plus outbox insert.
- Repository tests verify due-row ordering, publish-success deletion, and retry scheduling.
- Dequeuer unit tests use a fake publisher for success, transient failure, and exhausted retry behavior.
- Pub/Sub/emulator integration test verifies payload and attributes if the concrete Pub/Sub client lands in this slice.

## QA Guide

1. Run migrations.
2. Trigger a domain write that emits an event.
3. Query `outbox_messages` and confirm a due row exists after commit.
4. Run the dequeuer and confirm the row is deleted after publish succeeds.
