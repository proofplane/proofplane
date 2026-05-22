# Local Development

## Dependencies

Start local dependencies:

```bash
make up
```

Check readiness:

```bash
make health
```

Stop dependencies:

```bash
make down
```

Destroy local dependency state and recreate it:

```bash
make reset-local
```

## Services

- Postgres: `127.0.0.1:5432`
- Pub/Sub emulator: `127.0.0.1:8085`
- SpiceDB gRPC: `127.0.0.1:50051`

The local config file is `config/local.yaml`. It sets the defaults used by the `Makefile` through `PROOFPLANE_CONFIG`.

SpiceDB persists its own data in the `proofplane_spicedb` database on local Postgres. `make up` runs an idempotent database create step and SpiceDB datastore migration before starting the SpiceDB server, so an existing Postgres volume can be reused.

## Object Storage

Object storage is not run in Docker Compose for the MVP. Local configuration reserves `.local/storage` for the filesystem-backed object storage adapter planned in story 014.
