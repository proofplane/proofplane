# 011 - Pub/Sub Client and Subscription Runtime

## Goal

Implement Google Cloud Pub/Sub integration with local emulator support, automatic reconnection, and dead-letter handling.

## Design

Create static-dispatch traits for publishing and subscribing:

```rust
pub trait MessagePublisher {
    async fn publish(&self, topic: TopicName, message: OutboundMessage) -> Result<MessageId, PubSubError>;
}

pub trait MessageSubscriber {
    async fn receive(&self) -> Result<ReceivedMessage, PubSubError>;
}
```

Use concrete implementations for Google Pub/Sub/emulator. Support:

- topic provisioning for local/dev/test
- subscription provisioning
- automatic reconnection after emulator or network interruption
- ack and nack
- dead-letter topic publishing when a message fully exhausts delivery attempts

## Acceptance Criteria

- Pub/Sub client can run against local emulator through config.
- Subscribers recover from transient connection failures.
- Message attributes preserve event type, aggregate ID, causation ID, correlation ID, and attempt metadata.
- Dead-letter messages include original payload and failure metadata.
- Retry classification uses the shared `Retryable` trait.

## Tests

- Unit tests with fake publisher/subscriber cover ack, nack, retry, and dead-letter decisions.
- Integration tests publish and receive a message through the emulator.
- Integration tests simulate subscriber restart and verify messages continue flowing.
- Integration tests verify a permanently failing message is dead-lettered.

## QA Guide

1. Start Pub/Sub emulator.
2. Provision a topic, subscription, and dead-letter topic.
3. Publish a test message and confirm subscriber receives and acks it.
4. Force a handler failure until attempts are exhausted and inspect the dead-letter topic.
