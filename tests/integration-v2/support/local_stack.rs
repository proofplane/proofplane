use std::time::{Duration, Instant};

use proofplane::persistence;
use tokio::net::TcpStream;
use tokio_postgres::Client;

const POSTGRES_ADDRESS: &str = "127.0.0.1:5432";
const POSTGRES_ADMIN_URL: &str = "postgres://proofplane:proofplane@127.0.0.1:5432/postgres";
/// PgBouncer, in transaction mode, standing in for the Supavisor pooler that
/// production runtime traffic goes through.
const PGBOUNCER_ADDRESS: &str = "127.0.0.1:6432";
const SUITE_DATABASE: &str = "proofplane_integration_v2";
pub const PUBSUB_EMULATOR_ADDRESS: &str = "127.0.0.1:8086";

/// Nothing listening means the stack is down, and waiting will not change that.
/// The allowance is only for the moment between `docker compose up` returning
/// and the port being published.
const LISTEN_TIMEOUT: Duration = Duration::from_secs(3);
/// A published port that refuses to answer queries is a server still starting,
/// which is worth waiting out.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_millis(250);

/// An empty database for this run, and the URL that reaches it.
///
/// Dropped and recreated every run because the suite cannot tolerate rows from
/// the one before it: `reference_data` seeds fixed UUIDs, and every test claims
/// a fixed `auth0_sub` and workspace name against `UNIQUE` columns.
/// The URL is the pooler's, but the drop and create are not: `DROP DATABASE`
/// and `CREATE DATABASE` cannot run through a transaction pooler, and dropping
/// a database out from under PgBouncer requires reaching Postgres directly.
///
/// The drop terminates the server connections PgBouncer was holding to the old
/// suite database, so it logs one `got packet 'E' from server when not linked`
/// per pooled connection here. That is expected and harmless: PgBouncer
/// reconnects on the next checkout, and the database is recreated before the
/// application under test ever connects.
pub async fn reset_suite_database() -> Result<String, String> {
    let admin = wait_for_postgres().await?;
    wait_for_listener("PgBouncer", PGBOUNCER_ADDRESS).await?;

    admin
        .execute(
            &format!("DROP DATABASE IF EXISTS {SUITE_DATABASE} WITH (FORCE)"),
            &[],
        )
        .await
        .expect("previous suite database drops");
    admin
        .execute(&format!("CREATE DATABASE {SUITE_DATABASE}"), &[])
        .await
        .expect("suite database is created");

    Ok(format!(
        "postgres://proofplane:proofplane@{PGBOUNCER_ADDRESS}/{SUITE_DATABASE}"
    ))
}

pub async fn wait_for_pubsub() -> Result<(), String> {
    wait_for_listener("The Pub/Sub emulator", PUBSUB_EMULATOR_ADDRESS).await
}

async fn wait_for_postgres() -> Result<Client, String> {
    wait_for_listener("Postgres", POSTGRES_ADDRESS).await?;

    let deadline = Instant::now() + READY_TIMEOUT;
    let mut failure;

    loop {
        match open_admin_connection().await {
            Ok(client) => return Ok(client),
            Err(error) => failure = error,
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "\n\nPostgres is listening on {POSTGRES_ADDRESS} but never became ready.\n\n\
                 Last error: {failure}\n"
            ));
        }
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

async fn open_admin_connection() -> Result<Client, String> {
    let client = persistence::conn(POSTGRES_ADMIN_URL)
        .await
        .map_err(|error| error.to_string())?;
    client
        .simple_query("SELECT 1")
        .await
        .map_err(|error| error.to_string())?;

    Ok(client)
}

async fn wait_for_listener(service: &str, address: &str) -> Result<(), String> {
    let deadline = Instant::now() + LISTEN_TIMEOUT;
    let mut failure;

    loop {
        match TcpStream::connect(address).await {
            Ok(_) => return Ok(()),
            Err(error) => failure = error.to_string(),
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "\n\n{service} is not reachable at {address}.\n\n\
                 integration-v2 runs against the local stack rather than starting its own.\n\
                 Start it with:  make up && make health\n\n\
                 Last error: {failure}\n"
            ));
        }
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}
