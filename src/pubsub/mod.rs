use std::{collections::BTreeMap, future::Future};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicName(String);

impl TopicName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageId(String);

impl MessageId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub data: Vec<u8>,
    pub attributes: BTreeMap<String, String>,
}

impl OutboundMessage {
    pub fn new(data: impl Into<Vec<u8>>, attributes: BTreeMap<String, String>) -> Self {
        Self {
            data: data.into(),
            attributes,
        }
    }
}

#[derive(Debug, Error)]
pub enum PubSubError {
    #[error("publish failed: {0}")]
    Publish(String),
}

pub trait Publisher {
    fn publish(
        &self,
        topic: &TopicName,
        message: OutboundMessage,
    ) -> impl Future<Output = Result<MessageId, PubSubError>> + Send;
}

#[derive(Debug, Clone, Default)]
pub struct UnavailablePublisher;

impl Publisher for UnavailablePublisher {
    async fn publish(
        &self,
        _topic: &TopicName,
        _message: OutboundMessage,
    ) -> Result<MessageId, PubSubError> {
        Err(PubSubError::Publish(
            "Pub/Sub publisher implementation is not configured".to_owned(),
        ))
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::{Arc, Mutex};

    use super::{MessageId, OutboundMessage, PubSubError, Publisher, TopicName};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PublishedMessage {
        pub topic: TopicName,
        pub message: OutboundMessage,
    }

    #[derive(Debug, Clone)]
    pub struct FakePublisher {
        published: Arc<Mutex<Vec<PublishedMessage>>>,
        failures: Arc<Mutex<Vec<String>>>,
    }

    impl FakePublisher {
        pub fn new() -> Self {
            Self {
                published: Arc::new(Mutex::new(Vec::new())),
                failures: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn fail_next(&self, error: impl Into<String>) {
            self.failures
                .lock()
                .expect("fake publisher failure lock")
                .push(error.into());
        }

        pub fn published(&self) -> Vec<PublishedMessage> {
            self.published
                .lock()
                .expect("fake publisher published lock")
                .clone()
        }
    }

    impl Default for FakePublisher {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Publisher for FakePublisher {
        async fn publish(
            &self,
            topic: &TopicName,
            message: OutboundMessage,
        ) -> Result<MessageId, PubSubError> {
            if let Some(error) = self
                .failures
                .lock()
                .expect("fake publisher failure lock")
                .pop()
            {
                return Err(PubSubError::Publish(error));
            }

            self.published
                .lock()
                .expect("fake publisher published lock")
                .push(PublishedMessage {
                    topic: topic.clone(),
                    message,
                });

            Ok(MessageId::new("fake-message-id"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{MessageId, OutboundMessage, Publisher, TopicName, UnavailablePublisher};

    #[test]
    fn stores_topic_name() {
        let topic = TopicName::new("events");

        assert_eq!(topic.as_str(), "events");
    }

    #[test]
    fn stores_message_id() {
        let id = MessageId::new("message-1");

        assert_eq!(id.as_str(), "message-1");
    }

    #[test]
    fn stores_outbound_message() {
        let message = OutboundMessage::new(b"{}".to_vec(), BTreeMap::new());

        assert_eq!(message.data, b"{}".to_vec());
        assert!(message.attributes.is_empty());
    }

    #[tokio::test]
    async fn unavailable_publisher_fails_closed() {
        let publisher = UnavailablePublisher;

        let error = publisher
            .publish(
                &TopicName::new("outbox"),
                OutboundMessage::new(b"{}", BTreeMap::new()),
            )
            .await
            .expect_err("unavailable publisher fails");

        assert_eq!(
            error.to_string(),
            "publish failed: Pub/Sub publisher implementation is not configured"
        );
    }
}
