use proofplane::{errors::full_message, migrate, VERSION};

#[tokio::main]
async fn main() {
    let report = match migrate::run().await {
        Ok(report) => report,
        Err(error) => {
            // The whole chain: refinery reports `db error` and leaves the
            // reason to Postgres, and a job's logs are all an operator gets.
            eprintln!("{}", full_message(&error));
            std::process::exit(1);
        }
    };

    let applied = report.applied_migrations();

    if applied.is_empty() {
        println!("Proofplane {VERSION}: database already at the latest revision, applied nothing");
        return;
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
}
