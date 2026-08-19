//! Disposable Pub/Sub resources for the local emulator.
//!
//! Terraform owns every production topic, subscription, dead-letter resource,
//! and delivery permission. The runtime publishes and never provisions. This
//! module is the one exception: the local emulator starts empty, so local
//! development and the integration-v2 suite create their own resources through
//! [`provision`], which refuses to run outside emulator mode.

use google_cloud_gax::grpc::Code;
use google_cloud_gax::retry::RetrySetting;
use google_cloud_googleapis::pubsub::v1::{
    DeadLetterPolicy, PushConfig, Subscription as InternalSubscription, UpdateSubscriptionRequest,
};
use google_cloud_pubsub::client::Client;
use google_cloud_pubsub::subscription::SubscriptionConfig;
use prost_types::FieldMask;

use crate::config::PubSubSubscriptionsConfig;

use super::{
    connect, require_emulator, sdk_provision_error, topic_path, ClientMode, PubSubError, TopicName,
    MESSAGE_BUS_TOPIC, WORKER_DEAD_LETTER_TOPIC,
};

/// Every topic the local stack needs, including the dead-letter topic the
/// worker subscription names in its policy.
pub fn application_topics() -> [TopicName; 2] {
    [
        TopicName::new(MESSAGE_BUS_TOPIC),
        TopicName::new(WORKER_DEAD_LETTER_TOPIC),
    ]
}

/// Creates the application topics and the worker push subscription in the
/// emulator. It is safe to run again: an existing topic stays, and an existing
/// subscription is reconciled to the requested push endpoint and dead-letter
/// policy.
pub async fn provision(
    project_id: &str,
    subscriptions: &PubSubSubscriptionsConfig,
) -> Result<(), PubSubError> {
    // The same variable the SDK reads, so the mode and the connection agree.
    let mode = ClientMode::from_env();
    require_emulator(&mode)?;

    let client = connect(project_id, &mode).await?;

    for topic in application_topics() {
        ensure_client_topic(&client, &topic).await?;
    }

    ensure_client_worker_subscription(
        &client,
        &WorkerSubscriptionConfig::from_config(project_id, subscriptions),
    )
    .await
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

async fn ensure_client_worker_subscription(
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

    if topic.exists(None).await.map_err(sdk_provision_error)? {
        return Ok(());
    }

    topic.create(None, None).await.map_err(sdk_provision_error)
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
mod tests {
    use super::{
        application_topics, subscription_config, topic_path, TopicName, WorkerSubscriptionConfig,
        MESSAGE_BUS_TOPIC, WORKER_DEAD_LETTER_TOPIC,
    };

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
