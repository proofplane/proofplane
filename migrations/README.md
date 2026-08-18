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
2. `PROOFPLANE_MIGRATION_DATABASE_URL` — the URL itself. This is the local path:
   `make migrate` sets it to Postgres on 5432. To migrate somewhere else, run
   `make migrate PROOFPLANE_MIGRATION_DATABASE_URL=…`.
3. `PROOFPLANE_CONFIG` — an application configuration file, whose
   `database.url` is used.

The command applies migrations and nothing else. It never writes seed or demo
data, and production never runs `seed`.

## Naming

`V<number>__<label>_<snake_case_name>.sql`, numbered in order, three digits by
convention: `V002__expand_add_control_owner.sql`. refinery records each applied
migration in `refinery_schema_history` with a checksum, so an already-applied
file must never be edited — correct it with a new migration instead.

Every migration carries one of two labels, and the label is load-bearing:

- `expand_` — an earlier release keeps working against this migration.
- `contract_` — an earlier release must not run against this migration.

The label is the whole reason a rollback can be judged safe. See
[Rollback](#rollback). `V001__contract_initial_schema.sql` is labeled
`contract_` because the label states rollback safety rather than DDL shape: no
binary predates the initial schema, so none may run behind it.

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

## Rollback

Runtime processes check the schema history at startup
(`persistence::check_schema_revision`) and refuse to serve a database they
cannot read correctly. The check accepts a database that runs *ahead* of the
binary, but only by migrations labeled `expand_`. That is what makes restoring
the previous release a supported recovery after a failed deploy.

The label has to carry this because a binary cannot judge it at runtime.
Migrations are embedded at compile time, so a binary built before `V011` never
sees its SQL — only the version, name, and checksum recorded in the history
table. The release that writes the migration is the only place that knows
whether an earlier release survives it, so it declares the answer in the name.

The rules the check applies:

- The database is behind the binary — refuse. New code may read a column that
  does not exist yet. Run `make migrate`.
- The database matches the binary — serve.
- The database is ahead by `expand_` migrations only — serve, and log a
  warning. This is the rollback state.
- The database is ahead by any `contract_` or unlabeled migration — refuse.
- The history diverges at a shared position — refuse. That is corruption or a
  forked history.

An unlabeled migration is refused because it is not *provably* additive. A
forgotten label therefore costs a rollback that would have been safe, and never
permits one that is not.

Nothing checks that a migration labeled `expand_` is really additive. That is a
review responsibility, and the label is part of what a migration review reads.

## Locks

The `migrate` command sets a five-second `lock_timeout`
(`persistence::MIGRATION_LOCK_TIMEOUT`) before it runs. refinery takes no
advisory lock of its own, so without that bound a migration meeting a
conflicting session would queue behind it — and every statement arriving after
it would queue too. A migration that cannot take its lock fails the job instead.

That bound is a session setting, so it holds only on a direct connection. A
transaction pooler gives each transaction whichever server connection is free,
so the setting would not reach the migration's own transactions. Production
connects the migration job to the database's direct endpoint for that reason.
`make migrate` connects to Postgres on 5432 rather than to PgBouncer on 6432, so
the local run matches.

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
