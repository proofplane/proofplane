use std::sync::LazyLock;

use chrono::Utc;
use google_cloud_pubsub::{
    client::{Client, ClientConfig},
    subscriber::ReceivedMessage,
    subscription::SubscriptionConfig,
};
use proofplane::{
    config::PubSubSubscriptionsConfig,
    dequeuer::{OutboxDequeuer, OutboxDequeuerConfig},
    pubsub::{
        ensure_worker_subscription, GoogleCloudPublisher, TopicName, MESSAGE_BUS_TOPIC,
        PUBSUB_EMULATOR_HOST, WORKER_DEAD_LETTER_TOPIC,
    },
    repository::{NewOutboxMessage, OutboxMessage, Postgres},
    store,
};
use serde_json::json;
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage,
};
use testcontainers_modules::postgres;
use uuid::Uuid;

const PROJECT_ID: &str = "proofplane-integration";
const SUBSCRIPTION: &str = "proofplane-worker-integration";
static PUBSUB_ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

#[tokio::test]
async fn dequeuer_publishes_outbox_rows_to_deltio_pubsub() {
    let _pubsub_env_lock = PUBSUB_ENV_LOCK.lock().await;
    let pubsub_container = start_deltio().await;
    let emulator_host = emulator_host(&pubsub_container).await;
    std::env::set_var(PUBSUB_EMULATOR_HOST, &emulator_host);

    let publisher = GoogleCloudPublisher::new(PROJECT_ID)
        .await
        .expect("publisher builds");
    let pubsub_client = pubsub_client(PROJECT_ID).await;
    ensure_subscription(&pubsub_client, SUBSCRIPTION, MESSAGE_BUS_TOPIC)
        .await
        .expect("subscription exists");

    let postgres_container = postgres::Postgres::default()
        .start()
        .await
        .expect("Postgres test container starts");
    let database_url = postgres_url(&postgres_container).await;
    let mut database = store::conn(&database_url)
        .await
        .expect("fixture database connection opens");
    store::migrate(&mut database)
        .await
        .expect("database migrations run");
    drop(database);

    let pool = store::conn_pool(&database_url, 2)
        .await
        .expect("application Postgres pool opens");
    let postgres = Postgres::new(pool);
    let outbox_id = append_outbox_message(&postgres).await.id;

    let dequeuer = OutboxDequeuer::new(&postgres, &publisher, OutboxDequeuerConfig::default());

    assert_eq!(
        dequeuer
            .run_once(Utc::now() + chrono::Duration::seconds(1))
            .await
            .expect("dequeuer run succeeds"),
        1
    );
    assert!(postgres
        .list_due_outbox_messages(Utc::now(), 10)
        .await
        .expect("outbox rows list")
        .is_empty());

    let received = pull_one(&pubsub_client, SUBSCRIPTION).await;
    received.ack().await.expect("message acks");
    let message = received.message;

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&message.data).expect("message data is JSON"),
        json!({
            "outbox_message_id": outbox_id.to_string(),
            "event_type": "document.scan_requested",
            "aggregate_type": "evidence_document",
            "aggregate_id": "document-1",
            "request_id": null,
            "payload": { "scan_id": "scan-1" },
        })
    );
    assert!(message.attributes.is_empty());
}

#[tokio::test]
async fn worker_subscription_is_provisioned_and_reconciled_in_deltio() {
    let _pubsub_env_lock = PUBSUB_ENV_LOCK.lock().await;
    let pubsub_container = start_deltio().await;
    let emulator_host = emulator_host(&pubsub_container).await;
    std::env::set_var(PUBSUB_EMULATOR_HOST, &emulator_host);

    let subscription_id = format!("proofplane-worker-{}", Uuid::new_v4());
    let first_config = PubSubSubscriptionsConfig {
        worker: subscription_id.clone(),
        worker_push_endpoint: url::Url::parse("http://127.0.0.1:3001/pubsub/messages")
            .expect("push endpoint parses"),
        worker_max_delivery_attempts: 5,
    };

    ensure_worker_subscription(PROJECT_ID, &first_config)
        .await
        .expect("worker subscription is created");

    let pubsub_client = pubsub_client(PROJECT_ID).await;
    let subscription = pubsub_client.subscription(&subscription_id);
    let (topic, config) = subscription
        .config(None)
        .await
        .expect("created subscription config loads");

    assert_eq!(
        topic,
        format!("projects/{PROJECT_ID}/topics/{MESSAGE_BUS_TOPIC}")
    );
    assert_eq!(
        config.push_config.expect("push config").push_endpoint,
        "http://127.0.0.1:3001/pubsub/messages"
    );
    let dead_letter_policy = config.dead_letter_policy.expect("dead-letter policy");
    assert_eq!(
        dead_letter_policy.dead_letter_topic,
        format!("projects/{PROJECT_ID}/topics/{WORKER_DEAD_LETTER_TOPIC}")
    );
    assert_eq!(dead_letter_policy.max_delivery_attempts, 5);

    let reconciled_config = PubSubSubscriptionsConfig {
        worker: subscription_id.clone(),
        worker_push_endpoint: url::Url::parse("http://127.0.0.1:3002/pubsub/messages")
            .expect("push endpoint parses"),
        worker_max_delivery_attempts: 6,
    };
    ensure_worker_subscription(PROJECT_ID, &reconciled_config)
        .await
        .expect("worker subscription is reconciled");

    let (_, config) = subscription
        .config(None)
        .await
        .expect("updated subscription config loads");
    assert_eq!(
        config.push_config.expect("push config").push_endpoint,
        "http://127.0.0.1:3002/pubsub/messages"
    );
    assert_eq!(
        config
            .dead_letter_policy
            .expect("dead-letter policy")
            .max_delivery_attempts,
        6
    );
}

async fn start_deltio() -> ContainerAsync<GenericImage> {
    GenericImage::new("ghcr.io/jeffijoe/deltio", "latest")
        .with_exposed_port(8085.tcp())
        .with_wait_for(WaitFor::seconds(2))
        .start()
        .await
        .expect("deltio starts")
}

async fn emulator_host(container: &ContainerAsync<GenericImage>) -> String {
    let host = container.get_host().await.expect("deltio has a host");
    let port = container
        .get_host_port_ipv4(8085)
        .await
        .expect("deltio exposes Pub/Sub");

    format!("{host}:{port}")
}

async fn postgres_url(container: &ContainerAsync<postgres::Postgres>) -> String {
    let host = container.get_host().await.expect("Postgres has a host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Postgres exposes port");

    format!("postgres://postgres:postgres@{host}:{port}/postgres")
}

async fn pubsub_client(project_id: &str) -> Client {
    let config = ClientConfig {
        project_id: Some(project_id.to_owned()),
        ..Default::default()
    };
    Client::new(config).await.expect("Pub/Sub client builds")
}

async fn ensure_subscription(
    client: &Client,
    subscription_id: &str,
    topic_id: &str,
) -> Result<(), google_cloud_gax::grpc::Status> {
    let subscription = client.subscription(subscription_id);
    if subscription.exists(None).await? {
        return Ok(());
    }

    client
        .create_subscription(
            subscription_id,
            topic_id,
            SubscriptionConfig::default(),
            None,
        )
        .await
        .map(|_| ())
}

async fn append_outbox_message(postgres: &Postgres) -> OutboxMessage {
    let message = NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: "document.scan_requested".to_owned(),
        aggregate_type: "evidence_document".to_owned(),
        aggregate_id: "document-1".to_owned(),
        payload: json!({ "scan_id": "scan-1" }),
        request_id: None,
    };

    postgres
        .in_transaction(async move |context| context.append_outbox_message(&message).await)
        .await
        .expect("outbox message appends")
}

async fn pull_one(client: &Client, subscription_id: &str) -> ReceivedMessage {
    let subscription = client.subscription(subscription_id);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let mut messages = subscription.pull(1, None).await.expect("messages pull");
            if let Some(message) = messages.pop() {
                return message;
            }
        }
    })
    .await
    .expect("message arrives before timeout")
}
