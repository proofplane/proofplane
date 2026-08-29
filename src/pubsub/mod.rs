use std::env;

use async_trait::async_trait;
use google_cloud_gax::grpc::Status;
use google_cloud_googleapis::pubsub::v1::PubsubMessage;
use google_cloud_pubsub::client::{Client, ClientConfig};
use thiserror::Error;

pub mod emulator;

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
}

impl OutboundMessage {
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Self { data: data.into() }
    }
}

#[derive(Debug, Error)]
pub enum PubSubError {
    #[error("publish failed: {0}")]
    Publish(String),
    #[error("google application default credentials are not available: {0}")]
    Credentials(String),
    #[error("pubsub client connection failed: {0}")]
    Connection(String),
    #[error("resource provisioning failed: {0}")]
    Provision(String),
    #[error(
        "emulator provisioning needs the environment variable PUBSUB_EMULATOR_HOST; Terraform owns the production topics and subscriptions"
    )]
    EmulatorRequired,
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
pub const WORKER_DEAD_LETTER_TOPIC: &str = "proof.message_bus.dead_letter";

/// How a Pub/Sub client authenticates, following the SDK convention: the
/// emulator variable selects the emulator, and its absence selects Google
/// application default credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMode {
    Emulator { host: String },
    ApplicationDefaultCredentials,
}

impl ClientMode {
    pub fn from_env() -> Self {
        Self::from_emulator_host(env::var(PUBSUB_EMULATOR_HOST).ok())
    }

    fn from_emulator_host(host: Option<String>) -> Self {
        match host {
            Some(host) if !host.is_empty() => Self::Emulator { host },
            _ => Self::ApplicationDefaultCredentials,
        }
    }

    /// A stable label for startup logs. It names the mode and never a secret.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Emulator { .. } => "emulator",
            Self::ApplicationDefaultCredentials => "application-default-credentials",
        }
    }

    /// The emulator address, for a startup log field. Credential mode has none.
    pub fn emulator_host(&self) -> Option<&str> {
        match self {
            Self::Emulator { host } => Some(host),
            Self::ApplicationDefaultCredentials => None,
        }
    }
}

/// Opens a client in the requested mode. Credential mode must load application
/// default credentials here: the SDK default installs a no-op token source, and
/// every call it makes to Google Pub/Sub would then be unauthenticated.
async fn connect(project_id: &str, mode: &ClientMode) -> Result<Client, PubSubError> {
    let config = ClientConfig {
        project_id: Some(project_id.to_owned()),
        ..Default::default()
    };

    let config = match mode {
        ClientMode::Emulator { .. } => config,
        ClientMode::ApplicationDefaultCredentials => config
            .with_auth()
            .await
            .map_err(|error| PubSubError::Credentials(error.to_string()))?,
    };

    Client::new(config)
        .await
        .map_err(|error| PubSubError::Connection(error.to_string()))
}

#[derive(Debug, Clone)]
pub struct GoogleCloudPublisher {
    client: Client,
}

impl GoogleCloudPublisher {
    pub async fn new(project_id: &str, mode: &ClientMode) -> Result<Self, PubSubError> {
        Ok(Self {
            client: connect(project_id, mode).await?,
        })
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

pub fn topic_path(project_id: &str, topic: &TopicName) -> String {
    format!("projects/{}/topics/{}", project_id, topic.as_str())
}

/// Refuses any mode but the emulator. Terraform owns the production topics and
/// subscriptions, so provisioning may run against the emulator and nowhere else.
fn require_emulator(mode: &ClientMode) -> Result<(), PubSubError> {
    match mode {
        ClientMode::Emulator { .. } => Ok(()),
        ClientMode::ApplicationDefaultCredentials => Err(PubSubError::EmulatorRequired),
    }
}

fn to_google_message(message: OutboundMessage) -> PubsubMessage {
    PubsubMessage {
        data: message.data,
        ..Default::default()
    }
}

fn sdk_publish_error(error: Status) -> PubSubError {
    PubSubError::Publish(error.to_string())
}

fn sdk_provision_error(error: Status) -> PubSubError {
    PubSubError::Provision(error.to_string())
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
    use google_cloud_gax::grpc::Status;

    use super::{
        require_emulator, sdk_publish_error, to_google_message, topic_path, ClientMode, MessageId,
        OutboundMessage, MESSAGE_BUS_TOPIC,
    };
    use super::{PubSubError, TopicName};

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
        let message = OutboundMessage::new(b"{}".to_vec());

        assert_eq!(message.data, b"{}".to_vec());
    }

    #[test]
    fn formats_canonical_topic_path() {
        assert_eq!(
            topic_path("proofplane-local", &TopicName::new(MESSAGE_BUS_TOPIC)),
            "projects/proofplane-local/topics/proof.message_bus"
        );
    }

    #[test]
    fn converts_outbound_message_to_sdk_message_without_changing_payload() {
        let message = OutboundMessage::new(b"{\"id\":\"message-1\"}".to_vec());

        let sdk_message = to_google_message(message);

        assert_eq!(sdk_message.data, b"{\"id\":\"message-1\"}".to_vec());
        assert!(sdk_message.attributes.is_empty());
    }

    #[test]
    fn maps_sdk_publish_errors_to_pubsub_publish_errors() {
        let error = Status::unavailable("pubsub unavailable");

        assert!(matches!(sdk_publish_error(error), PubSubError::Publish(_)));
    }

    #[test]
    fn emulator_host_selects_emulator_mode() {
        let mode = ClientMode::from_emulator_host(Some("127.0.0.1:8086".to_owned()));

        assert_eq!(
            mode,
            ClientMode::Emulator {
                host: "127.0.0.1:8086".to_owned()
            }
        );
        assert_eq!(mode.as_str(), "emulator");
        assert_eq!(mode.emulator_host(), Some("127.0.0.1:8086"));
    }

    #[test]
    fn absent_emulator_host_selects_application_default_credentials() {
        let mode = ClientMode::from_emulator_host(None);

        assert_eq!(mode, ClientMode::ApplicationDefaultCredentials);
        assert_eq!(mode.as_str(), "application-default-credentials");
        assert_eq!(mode.emulator_host(), None);
    }

    #[test]
    fn empty_emulator_host_selects_application_default_credentials() {
        assert_eq!(
            ClientMode::from_emulator_host(Some(String::new())),
            ClientMode::ApplicationDefaultCredentials
        );
    }

    #[test]
    fn provisioning_accepts_emulator_mode() {
        let mode = ClientMode::Emulator {
            host: "127.0.0.1:8086".to_owned(),
        };

        assert!(require_emulator(&mode).is_ok());
    }

    #[test]
    fn provisioning_refuses_credential_mode() {
        let error = require_emulator(&ClientMode::ApplicationDefaultCredentials)
            .expect_err("credential mode may not provision");

        assert!(matches!(error, PubSubError::EmulatorRequired));
        assert!(error.to_string().contains("PUBSUB_EMULATOR_HOST"));
    }
}
