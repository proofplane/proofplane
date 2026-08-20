//! Creates the local Pub/Sub emulator resources the stack needs.
//!
//! `make up` runs this after the compose services start, because the emulator
//! keeps no state between runs. No runtime process provisions anything:
//! Terraform owns the production topics and subscriptions, and this command
//! never ships in the release image.

use proofplane::{config, pubsub::emulator, VERSION};

#[tokio::main]
async fn main() {
    let config = match config::load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{:#}", anyhow::Error::from(error));
            std::process::exit(1);
        }
    };

    if let Err(error) =
        emulator::provision(&config.pubsub.project_id, &config.pubsub.subscriptions).await
    {
        eprintln!("{:#}", anyhow::Error::from(error));
        std::process::exit(1);
    }

    println!("Proofplane {VERSION} local Pub/Sub emulator ready");
    println!("project: {}", config.pubsub.project_id);
    for topic in emulator::application_topics() {
        println!("topic: {}", topic.as_str());
    }
    println!(
        "subscription: {} pushes to {}",
        config.pubsub.subscriptions.worker, config.pubsub.subscriptions.worker_push_endpoint
    );
}
