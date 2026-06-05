# 013 - Pub/Sub Push Worker Handler Runtime

## Status

Implemented. Story 011 owns outbound Pub/Sub publishing and story 012 owns the
transactional outbox dequeuer. This story added the HTTP worker service that
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

The endpoint accepts the Pub/Sub push envelope, decodes the base64
self-describing message payload into an internal `WorkerMessage`, and
dispatches by `event_type`. Application metadata lives in the JSON payload, not
Pub/Sub attributes.

The internal message shape should be independent of the transport envelope:

```rust
pub struct WorkerMessage {
    pub message_id: String,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub request_id: Option<String>,
    pub payload: serde_json::Value,
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

The worker should also not be responsible for provisioning the push subscription
that invokes it. In scale-to-zero environments such as Cloud Run, the push
subscription must already exist before Pub/Sub can send a request that starts
the worker. The dequeuer process owns worker delivery resource provisioning
because it is the process publishing to the message bus.

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

This implementation uses unauthenticated push. Authenticated Cloud Run push with
a service account is deferred.

Deltio supports creating push subscriptions, but does not implement
`UpdateSubscription`. Dequeuer startup updates subscriptions in place when the
Pub/Sub backend supports it and falls back to delete/recreate for emulators that
return unimplemented, so local subscriptions are not left stale.

## Acceptance Criteria

- Worker binary starts from YAML config and serves an HTTP Pub/Sub push endpoint.
- Dequeuer startup provisions the worker push subscription before publishing
  outbox messages.
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
  handling, retryable handler failure, and route-level `204`/`500` behavior.
- Integration test provisions the message bus topic, worker dead-letter topic,
  and worker push subscription against Deltio through the dequeuer-owned
  provisioning helper, including the create and reconciliation path.
- End-to-end Deltio push delivery, retry, and dead-letter behavior remains a QA
  step because Deltio's push/update feature coverage is emulator behavior rather
  than application code.

## QA Guide

1. Start local dependencies, including Deltio.
2. Run the worker HTTP service with local config.
3. Run the dequeuer.
4. Insert or trigger an outbox row for a supported `event_type`.
5. Confirm the worker handles the pushed message and returns success.
6. Force a retryable handler failure and confirm Deltio retries, then
   dead-letters after the configured maximum delivery attempts.
