# 011 - Pub/Sub Client and Subscription Runtime

## Status

Partially complete. The outbound publisher slice is implemented for Google
Pub/Sub/emulator, including an application topic registry and startup topic
provisioning. The subscription runtime, ack/nack abstraction, reconnect loop,
and dead-letter handling remain future work.

## Goal

Implement Google Cloud Pub/Sub integration with local emulator support. The MVP
currently needs outbound publishing for the transactional outbox; inbound
subscription handling, automatic reconnection, and dead-letter handling belong
to the later worker-runtime slice.

## Design

Create static-dispatch traits for publishing and, when the worker runtime
lands, subscribing:

```rust
pub trait Publisher {
    async fn publish(&self, topic: &TopicName, message: OutboundMessage) -> Result<MessageId, PubSubError>;
}

pub trait MessageSubscriber {
    async fn receive(&self) -> Result<ReceivedMessage, PubSubError>;
}
```

The implemented publisher side uses a concrete Google Pub/Sub SDK client. Topic
names for application-owned resources are not configured in YAML; they live in
the central Pub/Sub registry:

- `MESSAGE_BUS_TOPIC = "proof.message_bus"`
- `application_topics()` returns every topic the application must ensure

`GoogleCloudPublisher::new(project_id)` creates the SDK client once and ensures
every registered application topic exists before returning. The outbox row still
stores the destination `TopicName`, but current callers use the registry topic
instead of ad hoc string literals.

Remaining subscription-runtime work should support:

- subscription provisioning, when subscriptions are introduced
- automatic reconnection after emulator or network interruption
- ack and nack
- dead-letter topic publishing when a message fully exhausts delivery attempts

## Acceptance Criteria

- Pub/Sub publisher can run against the local emulator using `PUBSUB_EMULATOR_HOST`
  and configured `pubsub.project_id`.
- Application topic names are hard-coded in the Pub/Sub registry, not YAML
  config.
- Publisher construction provisions every registered application topic and fails
  if topic existence/create calls fail.
- Published messages preserve payload bytes and stringified attributes.
- Subscriber recovery, ack/nack, retry loops, and dead-letter behavior remain
  deferred until the subscription worker runtime is implemented.

## Tests

- Unit tests cover topic names, message IDs, outbound-message conversion, topic
  path formatting, SDK error mapping, and the application topic registry.
- Dequeuer unit tests use the fake publisher for success and failure behavior.
- Integration tests verify the concrete publisher can publish an outbox row
  through the Pub/Sub emulator and that the payload/attributes are received.
- Subscriber restart and dead-letter tests remain deferred.

## QA Guide

1. Start Pub/Sub emulator.
2. Set `PUBSUB_EMULATOR_HOST` and run the dequeuer with local config.
3. Confirm publisher startup creates `proof.message_bus`.
4. Insert an outbox row for `proof.message_bus`.
5. Run the dequeuer and confirm a test subscription receives the published
   payload and attributes.
