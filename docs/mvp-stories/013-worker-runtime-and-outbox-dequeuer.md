# 013 - Pub/Sub Push Worker Handler Runtime

## Status

Planned. Story 011 owns outbound Pub/Sub publishing and story 012 owns the
transactional outbox dequeuer. This story owns the HTTP worker service that
receives Pub/Sub push deliveries in both local and live environments.

## Goal

Build a worker runtime that handles one Pub/Sub message per HTTP request. In
live environments, Google Pub/Sub push subscriptions invoke the Cloud Run worker
service. Locally, Deltio push subscriptions invoke the same worker HTTP endpoint
so local behavior matches production without a separate subscriber loop.

## Design

The worker is an HTTP service, not a production Pub/Sub subscriber. Pub/Sub owns
delivery, retry, and dead-letter routing through subscription configuration:

```text
API handler
  -> outbox row
  -> dequeuer publishes to proof.message_bus
  -> Pub/Sub push subscription POSTs to worker HTTP endpoint
  -> worker dispatches by event_type
  -> HTTP status controls ack/retry
```

Live delivery uses a Google Pub/Sub push subscription pointed at the Cloud Run
worker endpoint. Local delivery uses a Deltio push subscription pointed at the
locally running worker endpoint. Deltio is the local push relay: it receives the
published message through its subscription machinery and performs the HTTP POST
to the worker itself. The application should not include a separate local relay
process.

Both live and local subscriptions should be configured with the same logical
dead-letter policy, including a dead-letter topic and maximum delivery attempts.

The worker exposes a Pub/Sub push endpoint such as:

```text
POST /pubsub/messages
```

The endpoint accepts the Pub/Sub push envelope, decodes the base64 message
payload, reads metadata attributes added by the dequeuer, normalizes the request
into an internal `WorkerMessage`, and dispatches by `event_type`.

The internal message shape should be independent of the transport envelope:

```rust
pub struct WorkerMessage {
    pub message_id: String,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: serde_json::Value,
    pub attributes: BTreeMap<String, String>,
    pub delivery_attempt: Option<u32>,
}
```

The worker returns a successful push acknowledgement only after the handler
succeeds. For Pub/Sub push delivery, `102`, `200`, `201`, `202`, and `204` are
successful acknowledgements; any other status code causes retry. Prefer `204 No
Content` for successful handler completion.

Failure policy:

- malformed push envelope: return `204` after logging, because retrying will not
  repair the message
- unknown `event_type`: return `204` after logging, unless a later story needs
  unknown events to be dead-lettered for operational inspection
- retryable handler failure, such as temporary database, object-storage, or
  scanner unavailability: return `500`
- permanent domain failure: record/log the failure and return `204` unless the
  handler has a reason to let Pub/Sub dead-letter the message

The worker should not implement a pull loop, bounded-channel worker pool, or
local relay. Cloud Run concurrency provides the live request-level worker pool,
and Deltio push subscriptions provide local push parity.

## Pub/Sub Resources

Application topics remain hard-coded in the Pub/Sub registry from story 011.
This story should add the worker-side application resources that are needed for
delivery:

- message bus topic: `proof.message_bus`
- worker dead-letter topic: `proof.message_bus.dead_letter`
- worker subscription: subscribes to `proof.message_bus`

Subscription endpoint configuration differs by environment:

- live: push endpoint is the deployed Cloud Run worker URL
- local: push endpoint is the local worker URL reachable from Deltio, such as a
  Docker Compose service URL or `host.docker.internal`

For local development, the network shape determines the endpoint:

- Deltio and worker in Docker Compose: use the worker service name
- Deltio in Docker and worker on the host: use `host.docker.internal`
- Deltio and worker on the host: use `127.0.0.1`

The production push subscription should use authenticated push with a service
account that can invoke the Cloud Run worker service. Local Deltio push can use
unauthenticated HTTP unless Deltio authentication support is explicitly added to
the local setup.

## Acceptance Criteria

- Worker binary starts from YAML config and serves an HTTP Pub/Sub push endpoint.
- Worker endpoint accepts Pub/Sub push envelopes and normalizes them into
  `WorkerMessage`.
- Worker dispatches messages by `event_type` through statically wired handlers.
- Handler success returns `204 No Content`.
- Retryable handler failures return a non-2xx status so Pub/Sub retries.
- Malformed and permanently unprocessable messages do not retry forever by
  default.
- Local Deltio push subscription can invoke the same worker endpoint used by the
  live push subscription design.
- No local relay binary or app-owned local subscriber loop is introduced.
- Worker subscription is configured with a dead-letter topic and maximum
  delivery attempts in local and live environments.
- Worker exposes liveness, readiness, and metrics through the existing HTTP
  runtime patterns where practical.

## Tests

- Unit tests decode Pub/Sub push envelopes into `WorkerMessage`.
- Unit tests cover base64 payload decoding, JSON payload parsing, attribute
  extraction, and optional `deliveryAttempt`.
- Unit tests cover dispatch success, unknown event handling, malformed envelope
  handling, retryable handler failure, and permanent handler failure policy.
- Integration test publishes to Deltio and verifies the push subscription invokes
  the local worker endpoint.
- Integration test verifies worker `2xx` responses acknowledge messages.
- Integration test verifies worker non-2xx responses are retried and eventually
  dead-lettered by Deltio when the configured delivery attempts are exhausted.

## QA Guide

1. Start local dependencies, including Deltio.
2. Run the worker HTTP service with local config.
3. Configure the Deltio push subscription for `proof.message_bus` to call the
   local worker endpoint. Deltio should perform the HTTP push directly.
4. Run the dequeuer.
5. Insert or trigger an outbox row for a supported `event_type`.
6. Confirm the worker handles the pushed message and returns success.
7. Force a retryable handler failure and confirm Deltio retries, then
   dead-letters after the configured maximum delivery attempts.
