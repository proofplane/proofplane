use async_trait::async_trait;
use google_cloud_gax::grpc::{Code, Status};
use google_cloud_gax::retry::RetrySetting;
use google_cloud_googleapis::pubsub::v1::{
    DeadLetterPolicy, PubsubMessage, PushConfig, Subscription as InternalSubscription,
    UpdateSubscriptionRequest,
};
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::subscription::SubscriptionConfig;
use prost_types::FieldMask;
use thiserror::Error;

use crate::config::PubSubSubscriptionsConfig;

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
    #[error("resource provisioning failed: {0}")]
    Provision(String),
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

pub fn application_topics() -> [TopicName; 2] {
    [
        TopicName::new(MESSAGE_BUS_TOPIC),
        TopicName::new(WORKER_DEAD_LETTER_TOPIC),
    ]
}

#[derive(Debug, Clone)]
pub struct GoogleCloudPublisher {
    client: Client,
}

impl GoogleCloudPublisher {
    pub async fn new(project_id: impl Into<String>) -> Result<Self, PubSubError> {
        let config = ClientConfig {
            project_id: Some(project_id.into()),
            ..Default::default()
        };
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
    let config = ClientConfig {
        project_id: Some(project_id.to_owned()),
        ..Default::default()
    };
    let client = Client::new(config)
        .await
        .map_err(|error| PubSubError::Publish(error.to_string()))?;

    ensure_client_topic(&client, topic).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSubscriptionConfig {
    pub subscription_id: String,
    pub topic_id: String,
    pub push_endpoint: String,
    pub dead_letter_topic_path: String,
    pub max_delivery_attempts: i32,
}

impl WorkerSubscriptionConfig {
    pub fn from_config(project_id: &str, subscriptions: &PubSubSubscriptionsConfig) -> Self {
        Self {
            subscription_id: subscriptions.worker.clone(),
            topic_id: MESSAGE_BUS_TOPIC.to_owned(),
            push_endpoint: subscriptions.worker_push_endpoint.to_string(),
            dead_letter_topic_path: topic_path(
                project_id,
                &TopicName::new(WORKER_DEAD_LETTER_TOPIC),
            ),
            max_delivery_attempts: i32::from(subscriptions.worker_max_delivery_attempts),
        }
    }
}

pub async fn ensure_worker_subscription(
    project_id: &str,
    subscriptions: &PubSubSubscriptionsConfig,
) -> Result<(), PubSubError> {
    let client_config = ClientConfig {
        project_id: Some(project_id.to_owned()),
        ..Default::default()
    };
    let client = Client::new(client_config)
        .await
        .map_err(|error| PubSubError::Provision(error.to_string()))?;

    for topic in application_topics() {
        ensure_client_topic(&client, &topic).await?;
    }

    ensure_client_worker_subscription(
        &client,
        &WorkerSubscriptionConfig::from_config(project_id, subscriptions),
    )
    .await
}

pub async fn ensure_client_worker_subscription(
    client: &Client,
    config: &WorkerSubscriptionConfig,
) -> Result<(), PubSubError> {
    let subscription = client.subscription(&config.subscription_id);
    let desired = subscription_config(config);

    if !subscription
        .exists(Some(RetrySetting::default()))
        .await
        .map_err(sdk_provision_error)?
    {
        client
            .create_subscription(&config.subscription_id, &config.topic_id, desired, None)
            .await
            .map_err(sdk_provision_error)?;
        return Ok(());
    }

    let (_, current) = subscription
        .config(Some(RetrySetting::default()))
        .await
        .map_err(sdk_provision_error)?;
    if subscription_matches(&current, config) {
        return Ok(());
    }

    let request = UpdateSubscriptionRequest {
        subscription: Some(InternalSubscription {
            name: subscription.fully_qualified_name().to_owned(),
            push_config: Some(push_config(config)),
            dead_letter_policy: Some(dead_letter_policy(config)),
            ..Default::default()
        }),
        update_mask: Some(FieldMask {
            paths: vec!["push_config".to_owned(), "dead_letter_policy".to_owned()],
        }),
    };

    if let Err(error) = subscription
        .get_client()
        .update_subscription(request, Some(RetrySetting::default()))
        .await
    {
        if error.code() == Code::Unimplemented {
            subscription
                .delete(None)
                .await
                .map_err(sdk_provision_error)?;
            client
                .create_subscription(&config.subscription_id, &config.topic_id, desired, None)
                .await
                .map_err(sdk_provision_error)?;

            return Ok(());
        }

        return Err(sdk_provision_error(error));
    }

    Ok(())
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
        ..Default::default()
    }
}

fn sdk_publish_error(error: Status) -> PubSubError {
    PubSubError::Publish(error.to_string())
}

fn sdk_provision_error(error: Status) -> PubSubError {
    PubSubError::Provision(error.to_string())
}

fn subscription_config(config: &WorkerSubscriptionConfig) -> SubscriptionConfig {
    SubscriptionConfig {
        push_config: Some(push_config(config)),
        dead_letter_policy: Some(dead_letter_policy(config)),
        ..Default::default()
    }
}

fn push_config(config: &WorkerSubscriptionConfig) -> PushConfig {
    PushConfig {
        push_endpoint: config.push_endpoint.clone(),
        ..Default::default()
    }
}

fn dead_letter_policy(config: &WorkerSubscriptionConfig) -> DeadLetterPolicy {
    DeadLetterPolicy {
        dead_letter_topic: config.dead_letter_topic_path.clone(),
        max_delivery_attempts: config.max_delivery_attempts,
    }
}

fn subscription_matches(current: &SubscriptionConfig, desired: &WorkerSubscriptionConfig) -> bool {
    current
        .push_config
        .as_ref()
        .is_some_and(|push| push.push_endpoint == desired.push_endpoint)
        && current.dead_letter_policy.as_ref().is_some_and(|policy| {
            policy.dead_letter_topic == desired.dead_letter_topic_path
                && policy.max_delivery_attempts == desired.max_delivery_attempts
        })
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
        application_topics, sdk_publish_error, subscription_config, to_google_message, topic_path,
        MessageId, OutboundMessage, WorkerSubscriptionConfig, MESSAGE_BUS_TOPIC,
        WORKER_DEAD_LETTER_TOPIC,
    };
    use super::{PubSubError, TopicName};

    #[test]
    fn stores_topic_name() {
        let topic = TopicName::new("events");

        assert_eq!(topic.as_str(), "events");
    }

    #[test]
    fn application_topic_registry_contains_message_bus() {
        assert_eq!(
            application_topics(),
            [
                TopicName::new(MESSAGE_BUS_TOPIC),
                TopicName::new(WORKER_DEAD_LETTER_TOPIC)
            ]
        );
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
    fn builds_worker_subscription_config_with_push_and_dead_letter_policy() {
        let config = WorkerSubscriptionConfig {
            subscription_id: "proofplane-worker".to_owned(),
            topic_id: MESSAGE_BUS_TOPIC.to_owned(),
            push_endpoint: "http://host.docker.internal:3001/pubsub/messages".to_owned(),
            dead_letter_topic_path: topic_path(
                "proofplane-local",
                &TopicName::new(WORKER_DEAD_LETTER_TOPIC),
            ),
            max_delivery_attempts: 7,
        };

        let sdk_config = subscription_config(&config);

        assert_eq!(
            sdk_config.push_config.expect("push config").push_endpoint,
            "http://host.docker.internal:3001/pubsub/messages"
        );
        let dead_letter_policy = sdk_config.dead_letter_policy.expect("dead-letter policy");
        assert_eq!(
            dead_letter_policy.dead_letter_topic,
            "projects/proofplane-local/topics/proof.message_bus.dead_letter"
        );
        assert_eq!(dead_letter_policy.max_delivery_attempts, 7);
    }
}
