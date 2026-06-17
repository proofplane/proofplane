use proofplane::{seed, VERSION};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let summary = match seed::run().await {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    println!("Proofplane {VERSION} local seed complete");
    println!(
        "Seeded local workspaces, actors, API credential, actor permissions, demo evidence requests, SOC 2 controls, and demo evidence submission"
    );
    match summary.demo_attachment {
        seed::DemoAttachmentSeedStatus::Seeded => {
            println!("demo evidence attachment/object: seeded for filesystem storage");
        }
        seed::DemoAttachmentSeedStatus::SkippedNonFilesystemStorage => {
            println!("demo evidence attachment/object: skipped for non-filesystem storage");
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
    println!(
        "local system actor API key (rotated by this seed run): {}",
        summary.api_key
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
