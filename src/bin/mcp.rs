use proofplane::{config, mcp, observability, store, VERSION};
use secrecy::ExposeSecret;
use thiserror::Error;
use tracing::{debug, error, info};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        error!("{}", e);
        std::process::exit(1);
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error("postgres connection error")]
    StoreConnection(#[from] store::conn::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),
}

async fn run() -> Result<(), Error> {
    let config = match config::load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = observability::init_tracing(&config.observability) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    let mut client = store::conn(config.postgres.expose_secret()).await?;

    debug!("running migrations");
    store::migrate(&mut client).await?;
    debug!("done running migrations");

    info!(
        binary = "mcp",
        version = VERSION,
        "{}",
        mcp::startup_message()
    );

    Ok(())
}
