use google_cloud_pubsub::client::{Client, ClientConfig};

/// Deletes the worker subscription a previous run left behind.
pub async fn reset_worker_subscription(project_id: &str, subscription_id: &str) {
    let client = Client::new(ClientConfig {
        project_id: Some(project_id.to_owned()),
        ..Default::default()
    })
    .await
    .expect("Pub/Sub emulator client connects");

    let subscription = client.subscription(subscription_id);
    if subscription
        .exists(None)
        .await
        .expect("previous worker subscription is queried")
    {
        subscription
            .delete(None)
            .await
            .expect("previous worker subscription is deleted");
    }
}
