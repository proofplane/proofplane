//! Explicit CQRS application operations.
//!
//! Commands and queries are task-oriented values. Each operation is handled by
//! one concrete type with an inherent `handle` method; this module deliberately
//! has no handler trait, mediator, registry, or service locator.

pub mod commands;
pub mod queries;

mod execution_metadata;

pub use execution_metadata::ExecutionMetadata;
