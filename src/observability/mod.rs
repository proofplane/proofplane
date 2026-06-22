use std::{env, fmt, io};

use thiserror::Error;
use tracing_subscriber::filter::EnvFilter;

use crate::config::{LogFormat, ObservabilityConfig};

pub mod audit;

pub const RUST_LOG: &str = "RUST_LOG";
pub const PROOFPLANE_CLI_LOG: &str = "PROOFPLANE_CLI_LOG";

pub fn default_log_filter() -> &'static str {
    "info"
}

pub fn init_tracing(config: &ObservabilityConfig) -> Result<(), Error> {
    let filter = log_filter(config)?;

    match config.log_format {
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_file(false)
            .with_line_number(false)
            .with_writer(io::stderr)
            .with_env_filter(filter)
            .try_init()
            .map_err(Error::subscriber_init),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .pretty()
            .with_file(false)
            .with_line_number(false)
            .with_writer(io::stderr)
            .with_env_filter(filter)
            .try_init()
            .map_err(Error::subscriber_init),
    }
}

pub fn init_cli_tracing(config: &ObservabilityConfig) -> Result<(), Error> {
    if cli_tracing_enabled()? {
        init_tracing(config)?;
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid log filter `{filter}`: {message}")]
    InvalidFilter { filter: String, message: String },
    #[error("invalid environment variable `{name}`: {message}")]
    InvalidEnvironment { name: String, message: String },
    #[error("failed to initialize tracing subscriber: {0}")]
    SubscriberInit(String),
}

impl Error {
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

fn log_filter(config: &ObservabilityConfig) -> Result<EnvFilter, Error> {
    let filter = configured_log_filter(config)?;

    EnvFilter::try_new(&filter).map_err(|error| Error::invalid_filter(filter, error))
}

fn configured_log_filter(config: &ObservabilityConfig) -> Result<String, Error> {
    match env::var(RUST_LOG) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(config.default_filter.clone()),
        Err(env::VarError::NotUnicode(_)) => Err(Error::InvalidFilter {
            filter: RUST_LOG.to_owned(),
            message: "must be valid unicode".to_owned(),
        }),
    }
}

fn cli_tracing_enabled() -> Result<bool, Error> {
    match env::var(PROOFPLANE_CLI_LOG) {
        Ok(value) => Ok(matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )),
        Err(env::VarError::NotPresent) => Ok(false),
        Err(env::VarError::NotUnicode(_)) => Err(Error::InvalidEnvironment {
            name: PROOFPLANE_CLI_LOG.to_owned(),
            message: "must be valid unicode".to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, sync::Mutex};

    use crate::config::{LogFormat, ObservabilityConfig};

    use super::{
        cli_tracing_enabled, configured_log_filter, default_log_filter, log_filter, Error,
        PROOFPLANE_CLI_LOG, RUST_LOG,
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
            Error::InvalidFilter { ref filter, .. } if filter == "proofplane=definitely-not-a-level"
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
            Error::InvalidFilter { ref filter, .. } if filter == "proofplane=definitely-not-a-level"
        ));
        restore_rust_log(previous);
    }

    #[test]
    fn leaves_cli_tracing_disabled_by_default() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(PROOFPLANE_CLI_LOG).ok();
        env::remove_var(PROOFPLANE_CLI_LOG);

        let enabled = cli_tracing_enabled().expect("cli tracing env is readable");

        assert!(!enabled);
        restore_cli_log(previous);
    }

    #[test]
    fn enables_cli_tracing_for_truthy_values() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(PROOFPLANE_CLI_LOG).ok();

        for value in ["1", "true", "TRUE", "yes", " YES "] {
            env::set_var(PROOFPLANE_CLI_LOG, value);

            let enabled = cli_tracing_enabled().expect("cli tracing env is readable");

            assert!(enabled, "{value} should enable cli tracing");
        }

        restore_cli_log(previous);
    }

    #[test]
    fn ignores_non_truthy_cli_tracing_values() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(PROOFPLANE_CLI_LOG).ok();

        for value in ["0", "false", "no", "debug", ""] {
            env::set_var(PROOFPLANE_CLI_LOG, value);

            let enabled = cli_tracing_enabled().expect("cli tracing env is readable");

            assert!(!enabled, "{value} should not enable cli tracing");
        }

        restore_cli_log(previous);
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

    fn restore_cli_log(previous: Option<String>) {
        match previous {
            Some(previous) => env::set_var(PROOFPLANE_CLI_LOG, previous),
            None => env::remove_var(PROOFPLANE_CLI_LOG),
        }
    }
}
