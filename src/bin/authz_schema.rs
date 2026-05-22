use std::{fs, path::PathBuf};

use proofplane::{
    authorization::spicedb::{ClientError, SpiceDbClient},
    config, observability, VERSION,
};
use thiserror::Error;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        error!("{error}");
        std::process::exit(1);
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error("failed to read SpiceDB schema file {path}: {source}")]
    ReadSchema {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("SpiceDB error")]
    SpiceDb(#[from] ClientError),
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

    let schema_path = config.spicedb.schema_path.clone();
    let schema = fs::read_to_string(schema_path.clone()).map_err(|source| Error::ReadSchema {
        path: schema_path,
        source,
    })?;

    let client = SpiceDbClient::from_config(&config.spicedb).await?;
    client.write_schema(schema).await?;

    info!(
        binary = "authz-schema",
        version = VERSION,
        "SpiceDB schema applied"
    );

    Ok(())
}
