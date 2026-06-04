use chrono::Utc;
use google_cloud_pubsub::{
    client::{Client, ClientConfig},
    subscriber::ReceivedMessage,
    subscription::SubscriptionConfig,
};
use proofplane::{
    dequeuer::{OutboxDequeuer, OutboxDequeuerConfig},
    domain::{ActorId, WorkspaceId},
    pubsub::{GoogleCloudPublisher, TopicName, MESSAGE_BUS_TOPIC, PUBSUB_EMULATOR_HOST},
    repository::{NewOutboxMessage, OutboxMessage, Postgres},
    routes::authentication::ActorContext,
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

#[tokio::test]
async fn dequeuer_publishes_outbox_rows_to_deltio_pubsub() {
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
            .run_once(Utc::now())
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

    assert_eq!(message.data, br#"{"scan_id":"scan-1"}"#.to_vec());
    assert_eq!(message.attributes["source"], "integration-test");
    assert_eq!(message.attributes["priority"], "5");
    assert_eq!(
        message.attributes["outbox_message_id"],
        outbox_id.to_string()
    );
    assert_eq!(
        message.attributes["event_type"],
        "attachment.scan_requested"
    );
    assert_eq!(message.attributes["aggregate_type"], "evidence_attachment");
    assert_eq!(message.attributes["aggregate_id"], "attachment-1");
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
    let mut config = ClientConfig::default();
    config.project_id = Some(project_id.to_owned());
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
        event_type: "attachment.scan_requested".to_owned(),
        aggregate_type: "evidence_attachment".to_owned(),
        aggregate_id: "attachment-1".to_owned(),
        payload: json!({ "scan_id": "scan-1" }),
        attributes: json!({ "source": "integration-test", "priority": 5 }),
    };

    postgres
        .in_actor_context(actor_context(), async move |context| {
            context.append_outbox_message(&message).await
        })
        .await
        .expect("outbox message appends")
}

fn actor_context() -> ActorContext {
    ActorContext::new(
        WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000101").unwrap()),
        ActorId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000102").unwrap()),
    )
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
