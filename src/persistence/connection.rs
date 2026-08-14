use std::time::Duration;

use deadpool_postgres::{Hook, HookError, Pool, Runtime, Timeouts};
use thiserror::Error;
use tokio_postgres::{connect, Client, NoTls};
use tracing::{debug, error};

use crate::config::DatabasePoolConfig;

#[derive(Debug, Error)]
pub enum Error {
    #[error("postgres error: {0}")]
    TokioPostgres(#[from] tokio_postgres::Error),

    #[error("failed to build postgres pool")]
    Deadpool(#[from] deadpool_postgres::BuildError),
}

pub async fn conn(conn_str: &str) -> Result<Client, Error> {
    let (client, connection) = connect(conn_str, NoTls).await?;

    debug!("connected to postgres");

    tokio::spawn(async move {
        debug!("running connection");
        if let Err(e) = connection.await {
            error!("running connection returned error: {}", e);
        }
    });

    Ok(client)
}

/// Which runtime's pool size to take from the shared database configuration.
///
/// Production mounts one configuration document for every process, so the
/// process itself says which bound is its own.
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

/// How large a pool may grow and how long callers and connections may linger.
///
/// Every runtime reaches Postgres through a transaction pooler with a finite
/// client limit, so these are capacity decisions rather than tuning knobs.
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

    /// One runtime's bounds, read from the configuration every process shares.
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

pub async fn conn_pool(conn_str: &str, bounds: PoolBounds) -> Result<Pool, Error> {
    let config: tokio_postgres::Config = conn_str.parse()?;
    let mgr = deadpool_postgres::Manager::new(config, NoTls);

    Pool::builder(mgr)
        .max_size(bounds.max_size)
        // The bound applies to each phase, not to their sum: a caller that
        // waits for a slot and then opens a connection can spend up to twice
        // this before giving up. Recycling is bounded too, so no phase of
        // `get()` can block forever — today's `Fast` method only checks whether
        // the connection is closed, but a future `Verified` one would issue a
        // real round trip.
        .timeouts(Timeouts {
            wait: Some(bounds.acquire_timeout),
            create: Some(bounds.acquire_timeout),
            recycle: Some(bounds.acquire_timeout),
        })
        // deadpool has no idle reaper of its own. Rejecting a stale connection
        // here drops it and creates a fresh one instead; the error never
        // reaches the caller.
        //
        // `last_used` is the time since the connection was last handed out,
        // because deadpool records no timestamp for when one is returned. That
        // makes this a conservative approximation of idle time: a connection
        // held for a long time is replaced on its next checkout even though it
        // never sat idle. Discarding early is safe; keeping a dead connection
        // is not.
        .pre_recycle(Hook::sync_fn(move |_, metrics| {
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

    use super::{conn_pool, PoolBounds, PoolRuntime, UTILITY_POOL_SIZE};
    use crate::config::DatabasePoolConfig;
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

    #[tokio::test]
    async fn waiting_past_the_acquire_timeout_fails_instead_of_blocking() {
        let database = test_support::database().await;
        let pool = conn_pool(
            &database.url,
            PoolBounds::new(1, Duration::from_millis(100), Duration::from_secs(60)),
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
            PoolBounds::new(1, Duration::from_secs(5), Duration::from_secs(60)),
        )
        .await
        .expect("pool builds");

        let first = backend_pid(&pool).await;
        let second = backend_pid(&pool).await;

        assert_eq!(first, second, "an in-window connection is reused");
    }
}
