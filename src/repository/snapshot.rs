use deadpool_postgres::GenericClient;
use tokio_postgres::types::ToSql;

use super::Error;

pub(super) struct SnapshotMetadata {
    pub(super) table: &'static str,
    pub(super) conflict_column: &'static str,
    pub(super) scope_column: &'static str,
    pub(super) columns: &'static [&'static str],
}

/// A borrowed database representation of one aggregate's complete state.
///
/// `workspace_snapshot_record!` builds this from a private persistence record,
/// keeping the ordered SQL columns and their parameter values together. The
/// values are type-erased only at this Postgres boundary; the domain aggregate
/// and its record mapper remain strongly typed.
pub(super) struct WorkspaceSnapshot<'record> {
    metadata: SnapshotMetadata,
    values: Vec<&'record (dyn ToSql + Sync)>,
}

impl<'record> WorkspaceSnapshot<'record> {
    pub(super) fn new(
        metadata: SnapshotMetadata,
        values: Vec<&'record (dyn ToSql + Sync)>,
    ) -> Self {
        Self { metadata, values }
    }
}

/// Atomically inserts or replaces a workspace aggregate's complete snapshot.
pub(super) async fn save_workspace_snapshot(
    client: &impl GenericClient,
    snapshot: WorkspaceSnapshot<'_>,
) -> Result<(), Error> {
    let sql = build_upsert_sql(snapshot.metadata)?;
    let affected = client
        .execute(sql.as_str(), snapshot.values.as_slice())
        .await
        .map_err(super::constraints::classify_db_error)?;
    if affected != 1 {
        return Err(Error::InvariantViolation(
            "workspace snapshot save crossed a workspace boundary",
        ));
    }
    Ok(())
}

fn build_upsert_sql(metadata: SnapshotMetadata) -> Result<String, Error> {
    if !metadata.columns.contains(&metadata.conflict_column) {
        return Err(super::Error::InvariantViolation(
            "workspace snapshot must include its conflict column",
        ));
    }
    if !metadata.columns.contains(&metadata.scope_column) {
        return Err(super::Error::InvariantViolation(
            "workspace snapshot must include its scope column",
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

    // The transaction check proves that the incoming aggregate belongs to the
    // active workspace, but `ON CONFLICT` finds an existing row using only the
    // globally unique conflict key. If a bug supplied an ID already owned by a
    // different workspace, an unguarded update could overwrite that tenant's
    // row. Comparing the persisted and incoming scope columns makes that
    // collision affect zero rows, which `save_workspace_snapshot` rejects.
    Ok(format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} WHERE {}.{} = EXCLUDED.{}",
        metadata.table,
        metadata.columns.join(", "),
        placeholders,
        metadata.conflict_column,
        assignments,
        metadata.table,
        metadata.scope_column,
        metadata.scope_column,
    ))
}

macro_rules! workspace_snapshot_record {
    (
        struct $record:ident {
            $($column:ident: $column_type:ty),+ $(,)?
        }
        table: $table:ident,
        conflict: $conflict:ident,
        scope: $scope:ident $(,)?
    ) => {
        struct $record {
            $($column: $column_type),+
        }

        impl $record {
            fn as_workspace_snapshot(&self) -> super::snapshot::WorkspaceSnapshot<'_> {
                let metadata = super::snapshot::SnapshotMetadata {
                    table: stringify!($table),
                    conflict_column: stringify!($conflict),
                    scope_column: stringify!($scope),
                    columns: &[$(stringify!($column)),+],
                };
                let values = vec![$(&self.$column as &(dyn tokio_postgres::types::ToSql + Sync)),+];
                super::snapshot::WorkspaceSnapshot::new(metadata, values)
            }
        }
    };
}

pub(super) use workspace_snapshot_record;

#[cfg(test)]
mod tests {
    use super::{build_upsert_sql, SnapshotMetadata};

    #[test]
    fn builds_atomic_workspace_scoped_full_snapshot_upsert() {
        let sql = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            scope_column: "workspace_id",
            columns: &["id", "workspace_id", "name", "completed_at"],
        })
        .unwrap();

        assert_eq!(
            sql,
            "INSERT INTO example_aggregates (id, workspace_id, name, completed_at) \
VALUES ($1, $2, $3, $4) \
ON CONFLICT (id) DO UPDATE SET \
workspace_id = EXCLUDED.workspace_id, name = EXCLUDED.name, completed_at = EXCLUDED.completed_at \
WHERE example_aggregates.workspace_id = EXCLUDED.workspace_id"
        );
    }

    #[test]
    fn rejects_a_snapshot_without_its_workspace_scope_column() {
        let result = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            scope_column: "workspace_id",
            columns: &["id", "name"],
        });

        assert!(matches!(
            result,
            Err(crate::repository::Error::InvariantViolation(
                "workspace snapshot must include its scope column"
            ))
        ));
    }

    #[test]
    fn rejects_a_snapshot_without_its_conflict_column() {
        let result = build_upsert_sql(SnapshotMetadata {
            table: "example_aggregates",
            conflict_column: "id",
            scope_column: "workspace_id",
            columns: &["workspace_id", "name"],
        });

        assert!(matches!(
            result,
            Err(crate::repository::Error::InvariantViolation(
                "workspace snapshot must include its conflict column"
            ))
        ));
    }
}
