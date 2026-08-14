# Migrations

Schema migrations for the single Postgres database. They are embedded into every
binary at compile time by refinery (`src/persistence/migrate.rs`) and applied by
the `migrate` command.

## Applying them

```bash
make migrate   # applies migrations, writes no data
make seed      # applies migrations and then seeds local demo data
```

`make migrate` runs the `migrate` binary, which resolves its database URL from
the first of these that is set, and fails naming that source if it is set but
unusable:

1. `PROOFPLANE_MIGRATION_DATABASE_URL_FILE` — a path to a file holding the URL.
   This is what production uses: the deployment mounts the migration secret as a
   file and mounts no application configuration at all.
2. `PROOFPLANE_MIGRATION_DATABASE_URL` — the URL itself, for a one-off run.
3. `PROOFPLANE_CONFIG` — an application configuration file, whose
   `database.url` is used. This is the local path.

The command applies migrations and nothing else. It never writes seed or demo
data, and production never runs `seed`.

## Naming

`V<number>__<snake_case_name>.sql`, numbered in order, three digits by
convention: `V002__add_control_owner.sql`. refinery records each applied
migration in `refinery_schema_history` with a checksum, so an already-applied
file must never be edited — correct it with a new migration instead.

## Expand, then contract

A release must be compatible with the revision it replaces. Cloud Run updates
revisions gradually and rolls back by restoring the previous image, so the old
code and the new schema are live at the same time in both directions.

So every schema change is split:

- **Expand** — additive only, in the release that introduces the application
  code using it. Add columns as nullable or with a default, add new tables, add
  indexes, backfill. The previous revision must keep working against the
  expanded schema untouched.
- **Contract** — destructive steps, in a **later** release, only after every
  revision that depended on the old shape has drained. Dropping a column,
  dropping a table, tightening a constraint to `NOT NULL`, and renaming all
  belong here.

A rename is an expand and a contract, never a single `ALTER … RENAME`: add the
new column, write both, backfill, switch reads, then drop the old one in a later
release. Rolling an application regression back does not reverse an expand
migration, and must not need to.

Exceptions require an announced maintenance window.

## Locks

The `migrate` command sets a five-second `lock_timeout`
(`persistence::MIGRATION_LOCK_TIMEOUT`) before it runs. refinery takes no
advisory lock of its own, so without that bound a migration meeting a
conflicting session would queue behind it — and every statement arriving after
it would queue too. A migration that cannot take its lock fails the job instead.

That bound is a session setting, so it holds on a direct connection — which is
what production uses. Locally `config/local.yaml` points at PgBouncer in
transaction mode, which hands each transaction whichever server connection is
free, so the setting neither reaches the migration's own transactions nor
disappears afterwards: it stays on whichever pooled connection ran it until the
stack restarts. Harmless in development, and the reason `make down && make up`
is the way to be sure it is gone.

That bounds how long a migration *waits* for a lock, not how long it holds one.
Write DDL that takes its locks briefly:

- Add a constraint as `NOT VALID` first, then `VALIDATE CONSTRAINT` in a later
  migration. The validation pass takes a weaker lock and does not block writers.
- Do not change a column's type on a large table. That rewrites it under
  `ACCESS EXCLUSIVE`.
- Adding a column with a default is metadata-only in modern Postgres and is
  safe; backfilling one is not, so backfill in batches outside the migration.

refinery runs each migration file inside a transaction, so statements that
cannot run in a transaction block — `CREATE INDEX CONCURRENTLY` above all — are
not available here. An index large enough to need a concurrent build has to be
created deliberately outside this command.

See [Database And Migrations](../docs/epics/production-deployment/spec.md#database-and-migrations)
for the production deployment contract.
