use proofplane::{seed, VERSION};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let summary = match seed::run().await {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("{:#}", anyhow::Error::from(error));
            std::process::exit(1);
        }
    };

    println!("Proofplane {VERSION} local seed complete");
    println!(
        "Seeded fresh local workspaces, owner membership, local agent connection permissions, demo evidence, demo controls, demo policies, and demo evidence submission"
    );
    match summary.demo_document {
        seed::DemoDocumentSeedStatus::Seeded => {
            println!("demo evidence document/object: seeded for filesystem storage");
        }
        seed::DemoDocumentSeedStatus::SkippedNonFilesystemStorage => {
            println!("demo evidence document/object: skipped for non-filesystem storage");
        }
    }
    println!(
        "authorized workspace: {}",
        Uuid::from(local_authorized_workspace_id())
    );
    println!(
        "unauthorized workspace: {}",
        Uuid::from(local_unauthorized_workspace_id())
    );
}

fn local_authorized_workspace_id() -> proofplane::domain::WorkspaceId {
    proofplane::domain::WorkspaceId::from(
        Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
    )
}

fn local_unauthorized_workspace_id() -> proofplane::domain::WorkspaceId {
    proofplane::domain::WorkspaceId::from(
        Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
    )
}
