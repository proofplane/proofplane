use google_cloud_pubsub::client::{Client, ClientConfig};

/// Deletes the worker subscription a previous run left behind.
///
/// The emulator outlives the test binary now. A subscription that survived with
/// unacked messages in it would deliver them into this run, against a database
/// that was just recreated and knows nothing about them. Provisioning is
/// otherwise idempotent, so recreating the subscription from nothing is the
/// only part of Pub/Sub setup that a shared emulator adds.
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
