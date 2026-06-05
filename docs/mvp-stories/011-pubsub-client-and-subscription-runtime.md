# 011 - Pub/Sub Client and Push Subscription Provisioning

## Status

Implemented. The Pub/Sub client layer supports Google Pub/Sub/Deltio emulator
publishing, application topic provisioning, worker push subscription
provisioning, and dead-letter topic configuration. Proofplane does not own a
pull-subscriber runtime in the MVP; story 013 handles inbound delivery through
Pub/Sub push HTTP requests.

## Goal

Implement Google Cloud Pub/Sub integration with local emulator support. The MVP
needs outbound publishing for the transactional outbox and provisioning helpers
for the Pub/Sub push resources that deliver messages to the worker.

## Design

Create a static-dispatch trait for publishing:

```rust
pub trait Publisher {
    async fn publish(&self, topic: &TopicName, message: OutboundMessage) -> Result<MessageId, PubSubError>;
}
```

The implemented publisher side uses a concrete Google Pub/Sub SDK client.
Application-owned topic names are not configured in YAML; they live in the
central Pub/Sub registry:

- `MESSAGE_BUS_TOPIC = "proof.message_bus"`
- `WORKER_DEAD_LETTER_TOPIC = "proof.message_bus.dead_letter"`
- `application_topics()` returns every topic the application must ensure

`GoogleCloudPublisher::new(project_id)` creates the SDK client once and ensures
every registered application topic exists before returning. The outbox row still
stores the destination `TopicName`, but current callers use the registry topic
instead of ad hoc string literals.

Worker delivery uses Pub/Sub push subscriptions, not an application-owned pull
loop. The Pub/Sub module provides worker subscription provisioning from
configuration:

- worker subscription ID
- push endpoint
- dead-letter topic path
- maximum delivery attempts

The dequeuer process calls the provisioning helper before publishing because it
is always awake when events are being sent. The worker process only receives
HTTP push requests; it does not provision the subscription that wakes it.

Ack, retry, and dead-letter behavior are controlled by Pub/Sub push semantics:
the worker returns a successful HTTP status to ack and a non-2xx status for
retryable handler failures. There is intentionally no `ack`/`nack` abstraction,
pull receive loop, reconnect loop, or local relay process in application code.

## Acceptance Criteria

- Pub/Sub publisher can run against the local emulator using `PUBSUB_EMULATOR_HOST`
  and configured `pubsub.project_id`.
- Application topic names are hard-coded in the Pub/Sub registry, not YAML
  config.
- Publisher construction provisions every registered application topic and fails
  if topic existence/create calls fail.
- Published messages preserve payload bytes and do not use Pub/Sub attributes
  for application metadata.
- Worker push subscription provisioning creates or reconciles the subscription,
  push endpoint, dead-letter topic, and maximum delivery attempts.
- Local Deltio behavior is handled with the same push subscription shape as live
  Pub/Sub, without an application-owned local relay.
- Pull-subscriber recovery, ack/nack, and reconnect loops are explicitly out of
  scope for the MVP architecture.

## Tests

- Unit tests cover topic names, message IDs, outbound-message conversion, topic
  path formatting, SDK error mapping, and the application topic registry.
- Dequeuer unit tests use the fake publisher for success and failure behavior.
- Integration tests verify the concrete publisher can publish an outbox row
  through the Pub/Sub emulator and that the self-describing payload is received.
- Integration tests verify worker subscription provisioning and reconciliation
  against Deltio.
- End-to-end push delivery, retry, and dead-letter behavior remain QA/runtime
  coverage because those semantics belong to the Pub/Sub service.

## QA Guide

1. Start Pub/Sub emulator.
2. Set `PUBSUB_EMULATOR_HOST` and run the dequeuer with local config.
3. Confirm publisher startup creates `proof.message_bus` and
   `proof.message_bus.dead_letter`.
4. Confirm dequeuer startup provisions the configured worker push subscription.
5. Insert an outbox row for `proof.message_bus`.
6. Run the worker and dequeuer and confirm the worker receives the pushed
   self-describing payload.
