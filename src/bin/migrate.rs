use proofplane::{migrate, VERSION};

// Returning the error prints the whole source chain and exits 1. refinery
// reports `db error` and leaves the reason to Postgres, and a job's logs are all
// an operator gets.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let report = migrate::run().await?;

    let applied = report.applied_migrations();

    if applied.is_empty() {
        println!("Proofplane {VERSION}: database already at the latest revision, applied nothing");
        return Ok(());
    }

    println!(
        "Proofplane {VERSION}: applied {} {}",
        applied.len(),
        if applied.len() == 1 {
            "migration"
        } else {
            "migrations"
        }
    );
    for migration in applied {
        println!("  {migration}");
    }

    Ok(())
}
