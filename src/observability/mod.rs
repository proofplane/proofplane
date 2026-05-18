use std::{env, fmt, io};

use thiserror::Error;
use tracing_subscriber::filter::EnvFilter;

use crate::config::{LogFormat, ObservabilityConfig};

pub const RUST_LOG: &str = "RUST_LOG";

pub fn default_log_filter() -> &'static str {
    "info"
}

pub fn init_tracing(config: &ObservabilityConfig) -> Result<(), ObservabilityError> {
    let filter = log_filter(config)?;

    match config.log_format {
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_writer(io::stderr)
            .with_env_filter(filter)
            .try_init()
            .map_err(ObservabilityError::subscriber_init),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .pretty()
            .with_writer(io::stderr)
            .with_env_filter(filter)
            .try_init()
            .map_err(ObservabilityError::subscriber_init),
    }
}

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("invalid log filter `{filter}`: {message}")]
    InvalidFilter { filter: String, message: String },
    #[error("failed to initialize tracing subscriber: {0}")]
    SubscriberInit(String),
}

impl ObservabilityError {
    fn invalid_filter(filter: impl Into<String>, error: impl fmt::Display) -> Self {
        Self::InvalidFilter {
            filter: filter.into(),
            message: error.to_string(),
        }
    }

    fn subscriber_init(error: impl fmt::Display) -> Self {
        Self::SubscriberInit(error.to_string())
    }
}

fn log_filter(config: &ObservabilityConfig) -> Result<EnvFilter, ObservabilityError> {
    let filter = configured_log_filter(config)?;

    EnvFilter::try_new(&filter).map_err(|error| ObservabilityError::invalid_filter(filter, error))
}

fn configured_log_filter(config: &ObservabilityConfig) -> Result<String, ObservabilityError> {
    match env::var(RUST_LOG) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(config.default_filter.clone()),
        Err(env::VarError::NotUnicode(_)) => Err(ObservabilityError::InvalidFilter {
            filter: RUST_LOG.to_owned(),
            message: "must be valid unicode".to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, sync::Mutex};

    use crate::config::{LogFormat, ObservabilityConfig};

    use super::{
        configured_log_filter, default_log_filter, log_filter, ObservabilityError, RUST_LOG,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn has_default_log_filter() {
        assert_eq!(default_log_filter(), "info");
    }

    #[test]
    fn uses_configured_default_filter_when_rust_log_is_absent() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(RUST_LOG).ok();
        env::remove_var(RUST_LOG);
        let config = observability_config("debug");

        let filter = configured_log_filter(&config).expect("filter is configured");

        assert_eq!(filter, "debug");
        restore_rust_log(previous);
    }

    #[test]
    fn uses_rust_log_when_present() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(RUST_LOG).ok();
        env::set_var(RUST_LOG, "proofplane=trace");
        let config = observability_config("info");

        let filter = configured_log_filter(&config).expect("filter is configured");

        assert_eq!(filter, "proofplane=trace");
        restore_rust_log(previous);
    }

    #[test]
    fn rejects_invalid_rust_log_filter() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(RUST_LOG).ok();
        env::set_var(RUST_LOG, "proofplane=definitely-not-a-level");
        let config = observability_config("info");

        let error = log_filter(&config).expect_err("filter is invalid");

        assert!(matches!(
            error,
            ObservabilityError::InvalidFilter { ref filter, .. } if filter == "proofplane=definitely-not-a-level"
        ));
        restore_rust_log(previous);
    }

    #[test]
    fn rejects_invalid_configured_default_filter() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(RUST_LOG).ok();
        env::remove_var(RUST_LOG);
        let config = observability_config("proofplane=definitely-not-a-level");

        let error = log_filter(&config).expect_err("filter is invalid");

        assert!(matches!(
            error,
            ObservabilityError::InvalidFilter { ref filter, .. } if filter == "proofplane=definitely-not-a-level"
        ));
        restore_rust_log(previous);
    }

    fn observability_config(default_filter: impl Into<String>) -> ObservabilityConfig {
        ObservabilityConfig {
            log_format: LogFormat::Json,
            default_filter: default_filter.into(),
        }
    }

    fn restore_rust_log(previous: Option<String>) {
        match previous {
            Some(previous) => env::set_var(RUST_LOG, previous),
            None => env::remove_var(RUST_LOG),
        }
    }
}
