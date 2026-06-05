# 012 - Transactional Outbox Publish Dequeuer

## Status

Implemented. The database outbox, repository APIs, generic dequeuer, dequeuer
binary, Google Pub/Sub publisher integration, and emulator integration coverage
are in place. The outbox row continues to store a `TopicName`, while valid
application topics are sourced from the Pub/Sub registry.

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
- `request_id UUID`
- `attempt_count INTEGER NOT NULL DEFAULT 0`
- `next_available_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Repository APIs support appending outbox records inside existing transactions,
listing due rows in stable `next_available_at, id` order, deleting rows after
successful publish, listing exhausted rows, and recording publish failures by
incrementing `attempt_count` and scheduling `next_available_at`.

The Pub/Sub side exposes a static-dispatch publisher trait for `OutboundMessage`
and `TopicName`. The single-process dequeuer polls due rows, publishes a
self-describing JSON message envelope to the topic stored on the row, deletes
only after publish success, and retries failures with backoff while the row
remains in the table.

Application Pub/Sub topic names are not YAML configuration. Current app topics
come from the Pub/Sub registry, including `proof.message_bus` and
`proof.message_bus.dead_letter`. Future callers should enqueue outbox rows with
registry-derived `TopicName` values rather than string literals.

Duplicate publishes are acceptable if the process crashes after publish but before delete; downstream handlers must be idempotent. MVP assumes one dequeuer process, so there are intentionally no claim or lock columns.

## Acceptance Criteria

- Domain state changes and outbox insertions commit atomically.
- Due rows are listed in stable order by `next_available_at, id`.
- Successfully published records are deleted after publish returns success.
- Failed publishes increment `attempt_count` and schedule `next_available_at`;
  failure detail is emitted through logs.
- Outbox schema is migrated without delivery status, claim, lock, or
  published-at columns.
- Dequeuer startup constructs a concrete Google Pub/Sub publisher and lets the
  publisher ensure all application topics.

## Tests

- Repository integration tests verify atomic commit and rollback of domain write plus outbox insert.
- Repository tests verify due-row ordering, publish-success deletion, exhausted
  listing, and retry scheduling.
- Dequeuer unit tests use a fake publisher for success, transient failure, and exhausted retry behavior.
- Pub/Sub/emulator integration test verifies publisher construction provisions
  the application topic, the dequeuer publishes the self-describing message
  envelope, and the row is deleted.

## QA Guide

1. Run migrations.
2. Trigger a domain write that emits an event.
3. Query `outbox_messages` and confirm a due row exists after commit.
4. Ensure `PUBSUB_EMULATOR_HOST` is set.
5. Run the dequeuer and confirm the row is deleted after publish succeeds.
