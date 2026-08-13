//! Postgres types for bound parameters.
//!
//! Runtime traffic goes through a transaction pooler, which does not support
//! named prepared statements. `tokio_postgres` names every statement it creates
//! for `query`/`execute`, so this codebase uses `query_typed`/`execute_typed`
//! instead: those parse into the unnamed statement and send the whole exchange
//! under a single `Sync`, which is one round trip and safe to run outside a
//! transaction.
//!
//! The cost of those methods is that the caller states each parameter's Postgres
//! type. [`param`] recovers that from the Rust type, so call sites keep binding
//! values and the compiler supplies the types. A parameter whose type has no
//! [`PgParam`] mapping fails to compile rather than at runtime.

use chrono::{DateTime, Utc};
use tokio_postgres::types::{ToSql, Type};
use uuid::Uuid;

/// The Postgres type a Rust value binds as.
///
/// Every mapping here is unambiguous because the schema is: one string type
/// (`text`, never `varchar`), one JSON type (`jsonb`), one timestamp type
/// (`timestamptz`), and no enums or domains. Adding an impl means committing to
/// a column type, so check the schema before extending this.
pub(crate) trait PgParam {
    fn pg_type() -> Type;
}

/// Pairs a value with its Postgres type for `query_typed`/`execute_typed`.
///
/// The lifetime is explicit so borrowed parameters work: without it the trait
/// object defaults to `'static` and only owned values bind.
pub(crate) fn param<'value, T>(value: &'value T) -> (&'value (dyn ToSql + Sync + 'value), Type)
where
    T: PgParam + ToSql + Sync + 'value,
{
    (value, T::pg_type())
}

macro_rules! pg_param {
    ($($rust:ty => $pg:ident),+ $(,)?) => {
        $(
            impl PgParam for $rust {
                fn pg_type() -> Type {
                    Type::$pg
                }
            }
        )+
    };
}

pg_param! {
    Uuid => UUID,
    str => TEXT,
    String => TEXT,
    [u8] => BYTEA,
    Vec<u8> => BYTEA,
    Vec<Uuid> => UUID_ARRAY,
    Vec<String> => TEXT_ARRAY,
    serde_json::Value => JSONB,
    DateTime<Utc> => TIMESTAMPTZ,
    i64 => INT8,
    i32 => INT4,
    bool => BOOL,
}

/// A null still carries the type of the value it stands in for.
impl<T: PgParam> PgParam for Option<T> {
    fn pg_type() -> Type {
        T::pg_type()
    }
}

impl<T: PgParam + ?Sized> PgParam for &T {
    fn pg_type() -> Type {
        T::pg_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema has no `varchar`, no `json`, and no naive `timestamp`, so
    /// these three are the mappings most likely to be wrong and least likely to
    /// be noticed.
    #[test]
    fn strings_json_and_timestamps_map_to_the_types_the_schema_uses() {
        assert_eq!(String::pg_type(), Type::TEXT);
        assert_eq!(<&str>::pg_type(), Type::TEXT);
        assert_eq!(serde_json::Value::pg_type(), Type::JSONB);
        assert_eq!(DateTime::<Utc>::pg_type(), Type::TIMESTAMPTZ);
    }

    #[test]
    fn digests_are_bytea_and_ids_are_uuid() {
        assert_eq!(<&[u8]>::pg_type(), Type::BYTEA);
        assert_eq!(Vec::<u8>::pg_type(), Type::BYTEA);
        assert_eq!(Uuid::pg_type(), Type::UUID);
    }

    #[test]
    fn collections_map_to_array_types() {
        assert_eq!(Vec::<Uuid>::pg_type(), Type::UUID_ARRAY);
        assert_eq!(Vec::<String>::pg_type(), Type::TEXT_ARRAY);
    }

    #[test]
    fn nullable_and_borrowed_values_keep_the_underlying_type() {
        assert_eq!(Option::<Uuid>::pg_type(), Type::UUID);
        assert_eq!(Option::<DateTime<Utc>>::pg_type(), Type::TIMESTAMPTZ);
        assert_eq!(Option::<Vec<u8>>::pg_type(), Type::BYTEA);
        assert_eq!(Option::<&str>::pg_type(), Type::TEXT);
    }

    #[test]
    fn param_pairs_a_value_with_its_type() {
        let id = Uuid::nil();
        assert_eq!(param(&id).1, Type::UUID);
    }
}
