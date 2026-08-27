use std::{future::Future, pin::Pin, time::Duration};

use deadpool_postgres::{Hook, HookError, Pool, PoolError, Runtime, Timeouts};
use openssl::x509::X509;
use postgres_native_tls::MakeTlsConnector;
use thiserror::Error;
use tokio_postgres::{config::SslMode, Client, NoTls};
use tracing::{debug, error};

use crate::config::{DatabasePoolConfig, DatabaseTls, DatabaseTlsConfig};

#[derive(Debug, Error)]
pub enum Error {
    #[error("postgres error")]
    TokioPostgres(#[from] tokio_postgres::Error),

    #[error("failed to build postgres pool")]
    Deadpool(#[from] deadpool_postgres::BuildError),

    #[error(
        "failed to build the verifying TLS connector: the system certificate store must be \
         readable"
    )]
    Tls(#[source] native_tls::Error),

    /// Separate from the message above, which would send an operator to the
    /// system store when the fault is in the configured value.
    #[error("failed to read the configured database root certificate")]
    RootCertificatePem(#[source] openssl::error::ErrorStack),

    #[error("failed to trust the configured database root certificate")]
    RootCertificate(#[source] native_tls::Error),

    /// The mode belongs in the message. A refused handshake and a refused
    /// password look alike in the logs otherwise.
    #[error("failed to open a database connection with TLS mode `{tls}`")]
    Connect {
        tls: DatabaseTls,
        #[source]
        source: tokio_postgres::Error,
    },

    /// A timeout or a closed pool rather than a refused handshake. The mode
    /// stays in the message, because the operator asks the same question.
    #[error("failed to take a database connection with TLS mode `{tls}`")]
    PoolConnect {
        tls: DatabaseTls,
        #[source]
        source: PoolError,
    },
}

/// What a connection puts on the wire.
///
/// This is the one place that turns the configured mode into a transport. Every
/// other decision below reads this type rather than the mode.
enum Transport {
    Plaintext,
    /// A connector that verifies the certificate chain and the hostname. It
    /// checks them against the system certificate store, which is what
    /// `native_tls` does by default, and against any root the configuration
    /// adds.
    Verified(MakeTlsConnector),
}

impl Transport {
    fn build(tls: &DatabaseTlsConfig) -> Result<Self, Error> {
        match tls.mode {
            DatabaseTls::Disable => Ok(Self::Plaintext),
            DatabaseTls::VerifyFull => {
                let mut builder = native_tls::TlsConnector::builder();

                if let Some(pem) = &tls.root_certificate {
                    // Added to the system store rather than replacing it. An
                    // endpoint whose certificate already chains to a public root
                    // keeps working, so one deployment may hold both kinds.
                    for certificate in root_certificates(pem)? {
                        builder.add_root_certificate(certificate);
                    }
                }

                builder
                    .build()
                    .map(MakeTlsConnector::new)
                    .map(Self::Verified)
                    .map_err(Error::Tls)
            }
        }
    }

    fn ssl_mode(&self) -> SslMode {
        match self {
            Self::Plaintext => SslMode::Disable,
            Self::Verified(_) => SslMode::Require,
        }
    }
}

fn root_certificates(pem: &str) -> Result<Vec<native_tls::Certificate>, Error> {
    X509::stack_from_pem(pem.as_bytes())
        .map_err(Error::RootCertificatePem)?
        .into_iter()
        .map(|certificate| {
            let der = certificate.to_der().map_err(Error::RootCertificatePem)?;

            native_tls::Certificate::from_der(&der).map_err(Error::RootCertificate)
        })
        .collect()
}

fn prepared_config(conn_str: &str, transport: &Transport) -> Result<tokio_postgres::Config, Error> {
    let mut config: tokio_postgres::Config = conn_str.parse()?;

    config.ssl_mode(transport.ssl_mode());

    Ok(config)
}

/// A connection future with the connector type erased. The two transports give
/// two `Connection` types, and a box gives them one type to spawn.
type BoxedConnection = Pin<Box<dyn Future<Output = Result<(), tokio_postgres::Error>> + Send>>;

pub async fn conn(conn_str: &str, tls: &DatabaseTlsConfig) -> Result<Client, Error> {
    let transport = Transport::build(tls)?;
    let config = prepared_config(conn_str, &transport)?;
    let mode = tls.mode;

    let (client, connection): (Client, BoxedConnection) = match transport {
        Transport::Plaintext => {
            let (client, connection) = config
                .connect(NoTls)
                .await
                .map_err(|source| Error::Connect { tls: mode, source })?;

            (client, Box::pin(connection))
        }
        Transport::Verified(connector) => {
            let (client, connection) = config
                .connect(connector)
                .await
                .map_err(|source| Error::Connect { tls: mode, source })?;

            (client, Box::pin(connection))
        }
    };

    tokio::spawn(async move {
        debug!("running connection");
        if let Err(e) = connection.await {
            error!("running connection returned error: {}", e);
        }
    });

    debug!("connected to postgres");

    Ok(client)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolRuntime {
    Api,
    Mcp,
    Worker,
    Dequeuer,
    /// One-shot commands such as seeding. Not a configured runtime: these are
    /// local utilities that run once and exit.
    Utility,
}

/// The size a utility command's pool may reach. Small and fixed, because a
/// one-shot command has no steady-state footprint to plan for.
const UTILITY_POOL_SIZE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolBounds {
    max_size: usize,
    acquire_timeout: Duration,
    idle_timeout: Duration,
}

impl PoolBounds {
    pub const fn new(max_size: usize, acquire_timeout: Duration, idle_timeout: Duration) -> Self {
        Self {
            max_size,
            acquire_timeout,
            idle_timeout,
        }
    }

    pub fn from_config(config: &DatabasePoolConfig, runtime: PoolRuntime) -> Self {
        let max_size = match runtime {
            PoolRuntime::Api => config.api,
            PoolRuntime::Mcp => config.mcp,
            PoolRuntime::Worker => config.worker,
            PoolRuntime::Dequeuer => config.dequeuer,
            PoolRuntime::Utility => UTILITY_POOL_SIZE,
        };

        Self::new(
            max_size,
            Duration::from_millis(config.acquire_timeout_ms),
            Duration::from_millis(config.idle_timeout_ms),
        )
    }
}

pub async fn conn_pool(
    conn_str: &str,
    tls: &DatabaseTlsConfig,
    bounds: PoolBounds,
) -> Result<Pool, Error> {
    let transport = Transport::build(tls)?;
    let config = prepared_config(conn_str, &transport)?;
    // `Manager` boxes its connector, so both transports give the same type.
    let mgr = match transport {
        Transport::Plaintext => deadpool_postgres::Manager::new(config, NoTls),
        Transport::Verified(connector) => deadpool_postgres::Manager::new(config, connector),
    };

    let pool = Pool::builder(mgr)
        .max_size(bounds.max_size)
        // Apply the timeout bound to each phase to be more forgiving.
        .timeouts(Timeouts {
            wait: Some(bounds.acquire_timeout),
            create: Some(bounds.acquire_timeout),
            recycle: Some(bounds.acquire_timeout),
        })
        // deadpool has no idle reaper of its own. Rejecting a stale connection
        // here drops it and creates a fresh one instead.
        .pre_recycle(Hook::sync_fn(move |_, metrics| {
            // This is using the `last_used` time, which is the time that the
            // connection was returned to the pool. So what we're checking here is
            // that if the connection has been sitting idle in the pool too long,
            // we want a fresh one.
            if metrics.last_used() > bounds.idle_timeout {
                return Err(HookError::message("connection exceeded the idle timeout"));
            }

            Ok(())
        }))
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(Error::Deadpool)?;

    // A pool opens no connection when it is built. Take one here, so a refused
    // handshake fails under the mode that refused it. A `deadpool` error at the
    // first caller names neither the mode nor the certificate.
    let connection = pool.get().await.map_err(|error| match error {
        PoolError::Backend(source) => Error::Connect {
            tls: tls.mode,
            source,
        },
        source => Error::PoolConnect {
            tls: tls.mode,
            source,
        },
    })?;
    drop(connection);

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        conn_pool, prepared_config, PoolBounds, PoolRuntime, Transport, UTILITY_POOL_SIZE,
    };
    use tokio_postgres::config::SslMode;

    use crate::config::{DatabasePoolConfig, DatabaseTls, DatabaseTlsConfig};
    use crate::persistence::test_support;

    /// Deliberately all-different, so a selector that reads the wrong field
    /// cannot coincidentally return the right number.
    const DISTINCT_SIZES: DatabasePoolConfig = DatabasePoolConfig {
        api: 11,
        mcp: 22,
        worker: 33,
        dequeuer: 44,
        acquire_timeout_ms: 1_500,
        idle_timeout_ms: 60_000,
    };

    #[test]
    fn each_runtime_takes_its_own_configured_size() {
        let size = |runtime| PoolBounds::from_config(&DISTINCT_SIZES, runtime).max_size;

        assert_eq!(size(PoolRuntime::Api), 11);
        assert_eq!(size(PoolRuntime::Mcp), 22);
        assert_eq!(size(PoolRuntime::Worker), 33);
        assert_eq!(size(PoolRuntime::Dequeuer), 44);
        assert_eq!(size(PoolRuntime::Utility), UTILITY_POOL_SIZE);
    }

    #[test]
    fn every_runtime_shares_the_configured_timeouts() {
        for runtime in [
            PoolRuntime::Api,
            PoolRuntime::Mcp,
            PoolRuntime::Worker,
            PoolRuntime::Dequeuer,
            PoolRuntime::Utility,
        ] {
            let bounds = PoolBounds::from_config(&DISTINCT_SIZES, runtime);

            assert_eq!(bounds.acquire_timeout, Duration::from_millis(1_500));
            assert_eq!(bounds.idle_timeout, Duration::from_millis(60_000));
        }
    }

    /// Verification against the system certificate store. No server in these
    /// tests presents a certificate that store carries.
    fn system_roots_only() -> DatabaseTlsConfig {
        DatabaseTlsConfig {
            mode: DatabaseTls::VerifyFull,
            root_certificate: None,
        }
    }

    /// Generous next to what the test below does, which is fail a handshake.
    const REJECTION_BOUNDS: PoolBounds =
        PoolBounds::new(1, Duration::from_secs(10), Duration::from_secs(60));

    /// The whole error chain. Each layer states its own part, and the reason a
    /// transport was refused is at the bottom of it.
    fn chain(error: impl std::error::Error + Send + Sync + 'static) -> String {
        format!("{:#}", anyhow::Error::new(error))
    }

    /// The mode is set after the string is parsed, so `sslmode` in the string
    /// cannot lower the transport, and cannot raise it either. `Require` rather
    /// than `Prefer`, so a refusal is reported rather than quietly downgraded.
    #[test]
    fn the_configured_mode_outranks_the_connection_string() {
        let lowered = transport(&system_roots_only());
        let raised = transport(&DatabaseTlsConfig::DISABLED);

        let verified = prepared_config("postgres://user@host/db?sslmode=disable", &lowered)
            .expect("the connection string parses");
        let plaintext = prepared_config("postgres://user@host/db?sslmode=require", &raised)
            .expect("the connection string parses");

        assert_eq!(verified.get_ssl_mode(), SslMode::Require);
        assert_eq!(plaintext.get_ssl_mode(), SslMode::Disable);
    }

    fn transport(tls: &DatabaseTlsConfig) -> Transport {
        Transport::build(tls).expect("the transport builds")
    }

    /// The runtimes take their connections from a pool, so the pool has to
    /// refuse a server that serves no TLS.
    #[tokio::test]
    async fn a_pool_refuses_a_server_that_serves_no_tls() {
        let database = test_support::database().await;

        let error = conn_pool(&database.url, &system_roots_only(), REJECTION_BOUNDS)
            .await
            .expect_err("a server without TLS cannot serve a verified pool");

        let reason = chain(error);
        assert!(
            reason.contains("verify-full") && reason.contains("server does not support TLS"),
            "expected the refused transport to be named, got: {reason}"
        );
    }
}
