pub mod api;
pub mod config;
pub mod domain;
pub mod errors;
pub mod mcp;
pub mod migrations;
pub mod observability;
pub mod pubsub;
pub mod repositories;
pub mod services;
pub mod storage;
pub mod validation;
pub mod worker;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn package_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[cfg(test)]
mod tests {
    use super::{package_name, VERSION};

    #[test]
    fn exposes_package_metadata() {
        assert_eq!(package_name(), "proofplane");
        assert!(!VERSION.is_empty());
    }
}
