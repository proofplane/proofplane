pub mod app;
pub mod application;
pub mod authentication;
pub mod config;
pub mod dequeuer;
pub mod domain;
pub mod errors;
pub mod handlers;
pub mod mcp;
pub mod migrations;
pub mod object_storage;
pub mod observability;
pub mod projections;
pub mod pubsub;
pub mod repository;
pub mod routes;
pub mod scanner;
pub mod seed;
pub mod services;
pub mod store;
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
