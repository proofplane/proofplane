//! Read-side application operations.
//!
//! Query modules follow the same one-operation/one-concrete-handler convention
//! as commands, but read DTOs directly and never rehydrate mutable aggregates.

pub mod get_user;
