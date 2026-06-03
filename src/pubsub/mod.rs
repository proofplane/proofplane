use std::collections::BTreeMap;

use async_trait::async_trait;
use google_cloud_gax::grpc::Status;
use google_cloud_googleapis::pubsub::v1::PubsubMessage;
use google_cloud_pubsub::client::{Client, ClientConfig};
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

#[async_trait]
pub trait Publisher {
    async fn publish(
        &self,
        topic: &TopicName,
        message: OutboundMessage,
    ) -> Result<MessageId, PubSubError>;
}

pub const PUBSUB_EMULATOR_HOST: &str = "PUBSUB_EMULATOR_HOST";
pub const MESSAGE_BUS_TOPIC: &str = "proof.message_bus";

pub fn application_topics() -> [TopicName; 1] {
    [TopicName::new(MESSAGE_BUS_TOPIC)]
}

#[derive(Debug, Clone)]
pub struct GoogleCloudPublisher {
    client: Client,
}

impl GoogleCloudPublisher {
    pub async fn new(project_id: impl Into<String>) -> Result<Self, PubSubError> {
        let mut config = ClientConfig::default();
        config.project_id = Some(project_id.into());
        let client = Client::new(config)
            .await
            .map_err(|error| PubSubError::Publish(error.to_string()))?;

        for topic in application_topics() {
            ensure_client_topic(&client, &topic).await?;
        }

        Ok(Self { client })
    }
}

#[async_trait]
impl Publisher for GoogleCloudPublisher {
    async fn publish(
        &self,
        topic: &TopicName,
        message: OutboundMessage,
    ) -> Result<MessageId, PubSubError> {
        let publisher = self.client.topic(topic.as_str()).new_publisher(None);

        let mut message_ids = publisher
            .publish_immediately(vec![to_google_message(message)], None)
            .await
            .map_err(sdk_publish_error)?;
        let message_id = message_ids
            .pop()
            .ok_or_else(|| PubSubError::Publish("publish returned no message IDs".to_owned()))?;

        Ok(MessageId::new(message_id))
    }
}

pub async fn ensure_topic(project_id: &str, topic: &TopicName) -> Result<(), PubSubError> {
    let mut config = ClientConfig::default();
    config.project_id = Some(project_id.to_owned());
    let client = Client::new(config)
        .await
        .map_err(|error| PubSubError::Publish(error.to_string()))?;

    ensure_client_topic(&client, topic).await
}

async fn ensure_client_topic(client: &Client, topic: &TopicName) -> Result<(), PubSubError> {
    let topic = client.topic(topic.as_str());

    if topic.exists(None).await.map_err(sdk_publish_error)? {
        return Ok(());
    }

    topic.create(None, None).await.map_err(sdk_publish_error)
}

pub fn topic_path(project_id: &str, topic: &TopicName) -> String {
    format!("projects/{}/topics/{}", project_id, topic.as_str())
}

fn to_google_message(message: OutboundMessage) -> PubsubMessage {
    PubsubMessage {
        data: message.data,
        attributes: message.attributes.into_iter().collect(),
        ..Default::default()
    }
}

fn sdk_publish_error(error: Status) -> PubSubError {
    PubSubError::Publish(error.to_string())
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

    #[async_trait::async_trait]
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

    use google_cloud_gax::grpc::Status;

    use super::{
        application_topics, sdk_publish_error, to_google_message, topic_path, MessageId,
        OutboundMessage, MESSAGE_BUS_TOPIC,
    };
    use super::{PubSubError, TopicName};

    #[test]
    fn stores_topic_name() {
        let topic = TopicName::new("events");

        assert_eq!(topic.as_str(), "events");
    }

    #[test]
    fn application_topic_registry_contains_message_bus() {
        assert_eq!(application_topics(), [TopicName::new(MESSAGE_BUS_TOPIC)]);
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

    #[test]
    fn formats_canonical_topic_path() {
        assert_eq!(
            topic_path("proofplane-local", &TopicName::new(MESSAGE_BUS_TOPIC)),
            "projects/proofplane-local/topics/proof.message_bus"
        );
    }

    #[test]
    fn converts_outbound_message_to_sdk_message_without_changing_payload_or_attributes() {
        let message = OutboundMessage::new(
            b"{\"id\":\"message-1\"}".to_vec(),
            BTreeMap::from([
                (
                    "event_type".to_owned(),
                    "attachment.scan_requested".to_owned(),
                ),
                ("source".to_owned(), "outbox".to_owned()),
            ]),
        );

        let sdk_message = to_google_message(message);

        assert_eq!(sdk_message.data, b"{\"id\":\"message-1\"}".to_vec());
        assert_eq!(
            sdk_message.attributes["event_type"],
            "attachment.scan_requested"
        );
        assert_eq!(sdk_message.attributes["source"], "outbox");
    }

    #[test]
    fn maps_sdk_publish_errors_to_pubsub_publish_errors() {
        let error = Status::unavailable("pubsub unavailable");

        assert!(matches!(sdk_publish_error(error), PubSubError::Publish(_)));
    }
}
