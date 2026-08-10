use deadpool_postgres::GenericClient;
use std::collections::HashSet;
use tokio_postgres::types::ToSql;

use super::Error;

pub(super) struct SnapshotMetadata {
    pub(super) table: &'static str,
    pub(super) conflict_column: &'static str,
    pub(super) columns: &'static [&'static str],
}

/// A borrowed database representation of one aggregate's complete state.
///
/// `snapshot_record!` builds this from a private persistence record,
/// keeping the ordered SQL columns and their parameter values together. The
/// values are type-erased only at this Postgres boundary; the domain aggregate
/// and its record mapper remain strongly typed.
pub(super) struct SnapshotRecord<'record> {
    metadata: SnapshotMetadata,
    values: Vec<&'record (dyn ToSql + Sync)>,
}

impl<'record> SnapshotRecord<'record> {
    pub(super) fn new(
        metadata: SnapshotMetadata,
        values: Vec<&'record (dyn ToSql + Sync)>,
    ) -> Self {
        Self { metadata, values }
    }
}

/// Atomically inserts or replaces an aggregate's complete snapshot.
pub(super) async fn save_snapshot(
    client: &impl GenericClient,
    snapshot: SnapshotRecord<'_>,
) -> Result<(), Error> {
    let sql = build_upsert_sql(snapshot.metadata)?;
    let affected = client
        .execute(sql.as_str(), snapshot.values.as_slice())
        .await
        .map_err(super::constraints::classify_db_error)?;
    ensure_one_affected(affected)
}

fn build_upsert_sql(metadata: SnapshotMetadata) -> Result<String, Error> {
    if metadata
        .columns
        .iter()
        .filter(|column| **column == metadata.conflict_column)
        .count()
        != 1
    {
        return Err(super::Error::InvariantViolation(
            "snapshot must include its conflict column exactly once",
        ));
    }
    if metadata
        .columns
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .len()
        != metadata.columns.len()
    {
        return Err(super::Error::InvariantViolation(
            "snapshot columns must be unique",
        ));
    }
    if metadata.columns.len() < 2 {
        return Err(super::Error::InvariantViolation(
            "snapshot must include a non-conflict column",
        ));
    }
    let placeholders = (1..=metadata.columns.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let assignments = metadata
        .columns
        .iter()
        .filter(|column| **column != metadata.conflict_column)
        .map(|column| format!("{column} = EXCLUDED.{column}"))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
        metadata.table,
        metadata.columns.join(", "),
        placeholders,
        metadata.conflict_column,
        assignments,
    ))
}

fn ensure_one_affected(affected: u64) -> Result<(), Error> {
    if affected == 1 {
        Ok(())
    } else {
        Err(Error::InvariantViolation(
            "snapshot save must affect exactly one row",
        ))
    }
}

macro_rules! snapshot_record {
    (
        struct $record:ident {
            $($column:ident: $column_type:ty),+ $(,)?
        }
        table: $table:ident,
        conflict: $conflict:ident $(,)?
    ) => {
        struct $record {
            $($column: $column_type),+
        }

        impl $record {
            fn as_snapshot(&self) -> super::snapshot::SnapshotRecord<'_> {
                let metadata = super::snapshot::SnapshotMetadata {
                    table: stringify!($table),
                    conflict_column: stringify!($conflict),
                    columns: &[$(stringify!($column)),+],
                };
                let values = vec![$(&self.$column as &(dyn tokio_postgres::types::ToSql + Sync)),+];
                super::snapshot::SnapshotRecord::new(metadata, values)
            }
        }
    };
}

pub(super) use snapshot_record;

#[cfg(test)]
mod tests {
    use super::{build_upsert_sql, ensure_one_affected, SnapshotMetadata};

    #[test]
    fn builds_full_snapshot_upsert() {
        let sql = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            columns: &["id", "workspace_id", "name", "completed_at"],
        })
        .unwrap();

        assert_eq!(
            sql,
            "INSERT INTO example_aggregates (id, workspace_id, name, completed_at) \
VALUES ($1, $2, $3, $4) \
ON CONFLICT (id) DO UPDATE SET \
workspace_id = EXCLUDED.workspace_id, name = EXCLUDED.name, completed_at = EXCLUDED.completed_at"
        );
    }

    #[test]
    fn rejects_a_snapshot_without_its_conflict_column() {
        let result = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            columns: &["workspace_id", "name"],
        });

        assert!(matches!(
            result,
            Err(crate::persistence::Error::InvariantViolation(
                "snapshot must include its conflict column exactly once"
            ))
        ));
    }

    #[test]
    fn rejects_a_snapshot_without_any_state_to_update() {
        let result = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            columns: &["id"],
        });

        assert!(matches!(
            result,
            Err(crate::persistence::Error::InvariantViolation(
                "snapshot must include a non-conflict column"
            ))
        ));
    }

    #[test]
    fn rejects_duplicate_columns() {
        let result = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            columns: &["id", "name", "name"],
        });
        assert!(matches!(
            result,
            Err(crate::persistence::Error::InvariantViolation(
                "snapshot columns must be unique"
            ))
        ));
    }

    #[test]
    fn requires_exactly_one_affected_row() {
        assert!(ensure_one_affected(1).is_ok());
        assert!(ensure_one_affected(0).is_err());
        assert!(ensure_one_affected(2).is_err());
    }
}
