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

The local config file is `config/local.yaml`. It sets the defaults used by the `Makefile` through `PROOFPLANE_CONFIG`.

## Object Storage

Object storage is not run in Docker Compose for the MVP. Local development and automated tests use a filesystem-backed `ObjectStore` implementation rooted at `.local/storage`.
