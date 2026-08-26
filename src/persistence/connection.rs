use std::time::Duration;

use deadpool_postgres::{Hook, HookError, Pool, Runtime, Timeouts};
use openssl::x509::X509;
use postgres_native_tls::MakeTlsConnector;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_postgres::{config::SslMode, Client, Connection, NoTls};
use tracing::{debug, error, info};

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

fn spawn_connection<S, T>(connection: Connection<S, T>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        debug!("running connection");
        if let Err(e) = connection.await {
            error!("running connection returned error: {}", e);
        }
    });
}

pub async fn conn(conn_str: &str, tls: &DatabaseTlsConfig) -> Result<Client, Error> {
    let transport = Transport::build(tls)?;
    let config = prepared_config(conn_str, &transport)?;
    let mode = tls.mode;

    // The two arms differ only by connector type, which is a generic parameter,
    // so they cannot be one arm.
    let client = match transport {
        Transport::Plaintext => {
            let (client, connection) = config
                .connect(NoTls)
                .await
                .map_err(|source| Error::Connect { tls: mode, source })?;
            spawn_connection(connection);
            client
        }
        Transport::Verified(connector) => {
            let (client, connection) = config
                .connect(connector)
                .await
                .map_err(|source| Error::Connect { tls: mode, source })?;
            spawn_connection(connection);
            client
        }
    };

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

    // A pool opens no connection here. A refused handshake therefore surfaces at
    // the first `get`, as a `deadpool` error this module cannot wrap, and that
    // error does not name the mode. This line does, and it is the line before
    // the failure: every caller takes a connection as soon as the pool is built.
    info!(
        tls = %tls.mode,
        // A boolean, not the certificate. Whether a deployment added a private
        // root is the first thing to check when a handshake is refused.
        private_root = tls.root_certificate.is_some(),
        "opened postgres pool"
    );

    Pool::builder(mgr)
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
        .map_err(Error::Deadpool)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use deadpool_postgres::{PoolError, TimeoutType};

    use super::{conn, conn_pool, PoolBounds, PoolRuntime, UTILITY_POOL_SIZE};
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

    /// Generous next to what these tests do, which is either fail the handshake
    /// or run one statement.
    const REJECTION_BOUNDS: PoolBounds =
        PoolBounds::new(1, Duration::from_secs(10), Duration::from_secs(60));

    /// The whole error chain. Each layer states its own part, and the reason a
    /// transport was refused is at the bottom of it.
    fn chain(error: impl std::error::Error + Send + Sync + 'static) -> String {
        format!("{:#}", anyhow::Error::new(error))
    }

    /// The container serves no certificate, so a verified connection cannot be
    /// made against it. That makes it the control for both directions below.
    ///
    /// `sslmode=disable` in the string would connect in plaintext if the string
    /// decided. The configured mode decides instead, and `Require` rather than
    /// `Prefer`, so the refusal is reported rather than quietly downgraded.
    #[tokio::test]
    async fn a_connection_string_cannot_lower_the_configured_tls() {
        let database = test_support::database().await;

        let error = conn(
            &format!("{}?sslmode=disable", database.url),
            &system_roots_only(),
        )
        .await
        .expect_err("a server without TLS cannot serve a verified connection");

        let reason = chain(error);
        assert!(
            reason.contains("verify-full") && reason.contains("server does not support TLS"),
            "expected the refused transport to be named, got: {reason}"
        );
    }

    /// The other direction, and the control for the test above: a string that
    /// asks for TLS does not get it either.
    #[tokio::test]
    async fn a_connection_string_cannot_raise_the_configured_tls() {
        let database = test_support::database().await;

        let client = conn(
            &format!("{}?sslmode=require", database.url),
            &DatabaseTlsConfig::DISABLED,
        )
        .await
        .expect("the configured mode keeps the connection plaintext");

        let row = client
            .query_typed_one("SELECT 1", &[])
            .await
            .expect("the plaintext connection serves a query");

        assert_eq!(row.get::<_, i32>(0), 1);
    }

    /// The runtimes take their connections from a pool, so the pool has to
    /// refuse the same server the single connection above refuses.
    #[tokio::test]
    async fn a_pool_refuses_a_server_that_serves_no_tls() {
        let database = test_support::database().await;
        let pool = conn_pool(&database.url, &system_roots_only(), REJECTION_BOUNDS)
            .await
            .expect("pool builds");

        let error = pool
            .get()
            .await
            .expect_err("a server without TLS cannot serve a verified pool");

        let reason = chain(error);
        assert!(
            reason.contains("server does not support TLS"),
            "expected the refused transport to be the reason, got: {reason}"
        );
    }

    /// The backend process serving a pooled connection. A different value means
    /// the pool handed out a different physical connection.
    async fn backend_pid(pool: &deadpool_postgres::Pool) -> i32 {
        let client = pool.get().await.expect("a connection is available");
        let row = client
            .query_typed_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("backend pid query succeeds");
        row.get(0)
    }

    /// One second, not one tenth of it. The bound covers connection creation as
    /// well as the wait. Every database test in this module starts a container
    /// of its own, so a tighter bound reports a busy machine as a pool timeout.
    const WAIT_ALLOWANCE: Duration = Duration::from_secs(1);

    #[tokio::test]
    async fn waiting_past_the_acquire_timeout_fails_instead_of_blocking() {
        let database = test_support::database().await;
        let pool = conn_pool(
            &database.url,
            &DatabaseTlsConfig::DISABLED,
            PoolBounds::new(1, WAIT_ALLOWANCE, Duration::from_secs(60)),
        )
        .await
        .expect("pool builds");

        let _held = pool.get().await.expect("the one connection opens");
        let error = pool
            .get()
            .await
            .expect_err("a second caller cannot be served");

        assert!(matches!(error, PoolError::Timeout(TimeoutType::Wait)));
    }

    #[tokio::test]
    async fn connection_idle_past_the_idle_timeout_is_replaced() {
        let database = test_support::database().await;
        let pool = conn_pool(
            &database.url,
            &DatabaseTlsConfig::DISABLED,
            PoolBounds::new(1, Duration::from_secs(5), Duration::from_millis(50)),
        )
        .await
        .expect("pool builds");

        let first = backend_pid(&pool).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        let second = backend_pid(&pool).await;

        assert_ne!(
            first, second,
            "a connection left idle past the timeout is replaced, not reused"
        );
    }

    /// The control for the test above: without it, a pool that never reused
    /// anything would also pass.
    #[tokio::test]
    async fn connection_used_within_the_idle_timeout_is_reused() {
        let database = test_support::database().await;
        let pool = conn_pool(
            &database.url,
            &DatabaseTlsConfig::DISABLED,
            PoolBounds::new(1, Duration::from_secs(5), Duration::from_secs(60)),
        )
        .await
        .expect("pool builds");

        let first = backend_pid(&pool).await;
        let second = backend_pid(&pool).await;

        assert_eq!(first, second, "an in-window connection is reused");
    }
}
