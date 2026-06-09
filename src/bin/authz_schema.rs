use std::{fs, path::PathBuf};

use proofplane::{
    authorization::spicedb::{ClientError, SpiceDbClient},
    config, observability,
};
use thiserror::Error;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
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

    if let Err(error) = observability::init_cli_tracing(&config.observability) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    let schema =
        fs::read_to_string(&config.spicedb.schema_path).map_err(|source| Error::ReadSchema {
            path: config.spicedb.schema_path.clone(),
            source,
        })?;

    let client = SpiceDbClient::from_config(&config.spicedb).await?;
    client.write_schema(schema).await?;

    println!("SpiceDB schema applied");

    Ok(())
}
