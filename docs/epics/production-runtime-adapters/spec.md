# Production Runtime Adapters Spec

## Goal

Run the existing evidence pipeline against production Google Cloud services.
Local filesystem and Pub/Sub emulator behavior remain first-class development
paths, but runtime composition must not encode them as the only supported modes.

## Object Store Runtime

Keep `ObjectStore` as the adapter contract and add concrete implementations for
filesystem and GCS. Runtime dependencies use a concrete enum:

```rust
pub enum RuntimeObjectStore {
    Filesystem(FilesystemObjectStore),
    Gcs(GcsObjectStore),
}
```

The enum delegates `put`, `get`, `head`, `copy`, and `delete` without dynamic
dispatch. Services and handlers depend on the enum instead of the current
filesystem concrete type.

GCS configuration defines the bucket and object-key prefix. GCS always uses
Google application default credentials; the existing endpoint-override and
anonymous-credential fields are removed because Proofplane does not run a GCS
emulator. The configured prefix is prepended outside the logical
workspace-scoped `ObjectKey` and is never persisted in attachment rows.

Uploads and reads stream. Copy may use GCS server-side copy/rewrite, but must
verify destination metadata before finalization marks an attachment uploaded.

## Pub/Sub Runtime

`GoogleCloudPublisher` already uses the Google client library, but the dequeuer
currently refuses startup unless `PUBSUB_EMULATOR_HOST` is present. Remove that
guard. Client construction follows the SDK convention:

- emulator variable present: connect anonymously to the emulator;
- emulator variable absent: use application default credentials and the
  configured project.

Topic and push-subscription provisioning remains idempotent. Production
provisioning errors fail startup with actionable context.

Push endpoint authentication is deployment-owned for the MVP. The worker must
document how Cloud Run/IAM or an equivalent ingress policy restricts the
endpoint; application-level verification of Google-signed push tokens is a
follow-up unless deployment cannot enforce it.

## Configuration And Errors

Unsupported-backend errors disappear once GCS is implemented. Storage errors
distinguish not found, authentication/permission, unavailable/timeout, integrity
failure, and invalid keys well enough for stable API/worker decisions.

No credentials are accepted inline in YAML. Production uses workload identity
or application default credentials.

## Verification

Local integration tests use `FilesystemObjectStore` and require no cloud
credentials. CI has a dedicated GCS integration-test configuration with a real
test bucket and application default credentials. The same object-store contract
suite runs against filesystem locally and real GCS in CI, including upload,
head, read, copy, delete, prefix isolation, checksums, and cleanup.

GCS tests create unique per-run prefixes and delete their objects even after
failures. The CI identity is restricted to the test bucket. Unit tests may still
cover parsing and deterministic error mapping, but fakes do not substitute for
the real GCS integration suite.

Pub/Sub emulator integration tests remain because Pub/Sub local behavior is a
separate decision. A credential-free construction test proves the dequeuer no
longer requires the emulator variable; normal CI integration coverage verifies
the configured Pub/Sub mode without a separate deployment smoke test.

## Revisions

- 2026-06-11: Added the production Pub/Sub gap discovered while reconciling
  legacy stories 011-014 with runtime code.
- 2026-06-11: Removed the GCS emulator path. Local tests use filesystem storage;
  CI integration tests exercise the real GCS adapter and clean up isolated
  prefixes.
